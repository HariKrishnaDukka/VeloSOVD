//! Linux SocketCAN ISO-TP transport.
//!
//! The Linux kernel performs ISO-TP segmentation, reassembly and flow control.
//! This module passes complete UDS payloads to the transport-independent
//! UDS protocol engine.

use std::{
    ffi::CString,
    io, mem,
    os::fd::RawFd,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::uds_dispatcher::UdsDispatcherHandle;

const PF_CAN: libc::c_int = 29;
const CAN_ISOTP: libc::c_int = 6;
const POLL_TIMEOUT_MS: libc::c_int = 500;
const MAX_UDS_PAYLOAD: usize = 4096;

// Linux struct sockaddr_can:
// sa_family_t can_family;
// int can_ifindex;
// union { ... struct { u32 rx_id; u32 tx_id; } tp; } can_addr;
#[repr(C)]
struct SockAddrCan {
    can_family: libc::sa_family_t,
    can_ifindex: libc::c_int,
    address: [u8; 8],
}

fn last_os_error() -> io::Error {
    io::Error::last_os_error()
}

fn open_isotp_socket(interface: &str, rx_id: u32, tx_id: u32) -> io::Result<RawFd> {
    let interface = CString::new(interface)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid interface name"))?;

    // SAFETY: interface is a valid NUL-terminated C string.
    let ifindex = unsafe { libc::if_nametoindex(interface.as_ptr()) };

    if ifindex == 0 {
        return Err(last_os_error());
    }

    // CAN_ISOTP is a Linux PF_CAN datagram protocol.
    // SAFETY: socket has no pointer arguments.
    let fd = unsafe { libc::socket(PF_CAN, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, CAN_ISOTP) };

    if fd < 0 {
        return Err(last_os_error());
    }

    let mut address = SockAddrCan {
        can_family: PF_CAN as libc::sa_family_t,
        can_ifindex: ifindex as libc::c_int,
        address: [0; 8],
    };

    // Linux ISO-TP address layout: rx_id followed by tx_id.
    address.address[0..4].copy_from_slice(&rx_id.to_ne_bytes());
    address.address[4..8].copy_from_slice(&tx_id.to_ne_bytes());

    // SAFETY: address has the Linux sockaddr_can layout and valid lifetime.
    let result = unsafe {
        libc::bind(
            fd,
            &address as *const SockAddrCan as *const libc::sockaddr,
            mem::size_of::<SockAddrCan>() as libc::socklen_t,
        )
    };

    if result < 0 {
        let error = last_os_error();

        // SAFETY: fd was successfully created above.
        unsafe {
            libc::close(fd);
        }

        return Err(error);
    }

    Ok(fd)
}

fn process_socket(
    fd: RawFd,
    ecu_id: &str,
    dispatcher: &UdsDispatcherHandle,
    running: &AtomicBool,
) -> io::Result<()> {
    let mut poll_descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    let mut request = [0_u8; MAX_UDS_PAYLOAD];

    while running.load(Ordering::Acquire) {
        poll_descriptor.revents = 0;

        // SAFETY: poll_descriptor points to one valid pollfd.
        let poll_result = unsafe { libc::poll(&mut poll_descriptor, 1, POLL_TIMEOUT_MS) };

        if poll_result < 0 {
            let error = last_os_error();

            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }

            return Err(error);
        }

        if poll_result == 0 {
            continue;
        }

        let events = poll_descriptor.revents;

        if events & libc::POLLNVAL != 0 {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }

        // Process readable data first, even if another event is also set.
        // This avoids discarding a final readable ISO-TP datagram on hangup.
        if events & libc::POLLIN == 0 {
            if events & libc::POLLERR != 0 {
                let mut socket_error: libc::c_int = 0;
                let mut error_len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;

                // SAFETY: socket_error and error_len are valid writable arguments.
                let result = unsafe {
                    libc::getsockopt(
                        fd,
                        libc::SOL_SOCKET,
                        libc::SO_ERROR,
                        &mut socket_error as *mut libc::c_int as *mut libc::c_void,
                        &mut error_len,
                    )
                };

                if result < 0 {
                    return Err(last_os_error());
                }

                if socket_error != 0 {
                    return Err(io::Error::from_raw_os_error(socket_error));
                }

                return Err(io::Error::other(
                    "ISO-TP socket reported POLLERR without SO_ERROR",
                ));
            }

            if events & libc::POLLHUP != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "ISO-TP socket hangup",
                ));
            }

            return Err(io::Error::other(format!(
                "Unexpected ISO-TP poll events: {events:#x}"
            )));
        }

        // The kernel returns a complete reassembled ISO-TP payload.
        // SAFETY: request is a valid writable buffer.
        let received =
            unsafe { libc::read(fd, request.as_mut_ptr() as *mut libc::c_void, request.len()) };

        if received < 0 {
            let error = last_os_error();

            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }

            return Err(error);
        }

        if received == 0 {
            continue;
        }

        let payload = &request[..received as usize];

        println!(
            "[{}][UDS][RX] {}",
            ecu_id,
            payload
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        );

        let response = dispatcher
            .process_blocking(payload.to_vec())
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error))?;

        if let Some(response) = response {
            // SAFETY: response is a valid readable buffer for this call.
            let sent = unsafe {
                libc::write(fd, response.as_ptr() as *const libc::c_void, response.len())
            };

            if sent < 0 {
                return Err(last_os_error());
            }

            if sent as usize != response.len() {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Incomplete ISO-TP response write",
                ));
            }

            println!(
                "[{}][UDS][TX] {}",
                ecu_id,
                response
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        } else {
            println!("[{}][UDS] Positive response suppressed", ecu_id);
        }
    }

    Ok(())
}

/// Run the ISO-TP endpoint until the shared running flag becomes false.
///
/// This function is intended to run on a dedicated OS thread.
pub fn run_isotp_endpoint(
    ecu_id: &str,
    interface: &str,
    rx_id: u32,
    tx_id: u32,
    dispatcher: UdsDispatcherHandle,
    running: Arc<AtomicBool>,
) -> io::Result<()> {
    let fd = open_isotp_socket(interface, rx_id, tx_id)?;

    println!("[{ecu_id}][ISO-TP] Listening on {interface}: RX=0x{rx_id:03X}, TX=0x{tx_id:03X}");

    let result = process_socket(fd, ecu_id, &dispatcher, &running);

    // SAFETY: fd was opened by this function and is closed exactly once.
    unsafe {
        libc::close(fd);
    }

    result
}
