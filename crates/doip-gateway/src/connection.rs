use crate::routing::{ROUTING_RESPONSE_SUCCESS, RoutingActivationRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    AwaitingActivation,
    RoutingActive,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationError {
    ConnectionNotAwaitingActivation,
    InvalidReservedField,
    UnsupportedActivationType,
    InvalidTesterAddress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutingConnection {
    state: ConnectionState,
    tester_logical_address: Option<u16>,
    entity_logical_address: u16,
}

impl RoutingConnection {
    pub const fn new(entity_logical_address: u16) -> Self {
        Self {
            state: ConnectionState::AwaitingActivation,
            tester_logical_address: None,
            entity_logical_address,
        }
    }

    pub const fn state(&self) -> ConnectionState {
        self.state
    }

    pub const fn tester_logical_address(&self) -> Option<u16> {
        self.tester_logical_address
    }

    pub const fn entity_logical_address(&self) -> u16 {
        self.entity_logical_address
    }

    pub fn activate(&mut self, request: &RoutingActivationRequest) -> Result<u8, ActivationError> {
        if self.state != ConnectionState::AwaitingActivation {
            return Err(ActivationError::ConnectionNotAwaitingActivation);
        }

        if request.reserved != [0; 4] {
            return Err(ActivationError::InvalidReservedField);
        }

        if request.activation_type != 0x00 {
            return Err(ActivationError::UnsupportedActivationType);
        }

        if request.tester_logical_address == 0x0000 || request.tester_logical_address == 0xFFFF {
            return Err(ActivationError::InvalidTesterAddress);
        }

        self.tester_logical_address = Some(request.tester_logical_address);
        self.state = ConnectionState::RoutingActive;

        Ok(ROUTING_RESPONSE_SUCCESS)
    }

    pub fn close(&mut self) {
        self.state = ConnectionState::Closed;
        self.tester_logical_address = None;
    }

    pub const fn permits_diagnostic_messages(&self) -> bool {
        matches!(self.state, ConnectionState::RoutingActive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::RoutingActivationRequest;

    fn valid_request() -> RoutingActivationRequest {
        RoutingActivationRequest {
            tester_logical_address: 0x0E00,
            activation_type: 0x00,
            reserved: [0; 4],
        }
    }

    #[test]
    fn connection_starts_awaiting_activation() {
        let connection = RoutingConnection::new(0x1101);

        assert_eq!(connection.state(), ConnectionState::AwaitingActivation);
        assert!(!connection.permits_diagnostic_messages());
    }

    #[test]
    fn successful_activation_enables_diagnostics() {
        let mut connection = RoutingConnection::new(0x1101);

        let result = connection.activate(&valid_request());

        assert_eq!(result, Ok(ROUTING_RESPONSE_SUCCESS));
        assert_eq!(connection.state(), ConnectionState::RoutingActive);
        assert!(connection.permits_diagnostic_messages());
        assert_eq!(connection.tester_logical_address(), Some(0x0E00));
    }

    #[test]
    fn rejects_nonzero_reserved_field() {
        let mut connection = RoutingConnection::new(0x1101);
        let mut request = valid_request();
        request.reserved = [0, 0, 0, 1];

        assert_eq!(
            connection.activate(&request),
            Err(ActivationError::InvalidReservedField)
        );

        assert_eq!(connection.state(), ConnectionState::AwaitingActivation);
        assert_eq!(connection.tester_logical_address(), None);
    }

    #[test]
    fn rejects_unsupported_activation_type() {
        let mut connection = RoutingConnection::new(0x1101);
        let mut request = valid_request();
        request.activation_type = 0x01;

        assert_eq!(
            connection.activate(&request),
            Err(ActivationError::UnsupportedActivationType)
        );

        assert_eq!(connection.state(), ConnectionState::AwaitingActivation);
    }

    #[test]
    fn rejects_invalid_tester_address() {
        let mut connection = RoutingConnection::new(0x1101);
        let mut request = valid_request();
        request.tester_logical_address = 0xFFFF;

        assert_eq!(
            connection.activate(&request),
            Err(ActivationError::InvalidTesterAddress)
        );
    }

    #[test]
    fn rejects_second_activation() {
        let mut connection = RoutingConnection::new(0x1101);
        let request = valid_request();

        assert_eq!(connection.activate(&request), Ok(ROUTING_RESPONSE_SUCCESS));

        assert_eq!(
            connection.activate(&request),
            Err(ActivationError::ConnectionNotAwaitingActivation)
        );
    }

    #[test]
    fn closing_connection_disables_diagnostics() {
        let mut connection = RoutingConnection::new(0x1101);

        connection.activate(&valid_request()).unwrap();
        connection.close();

        assert_eq!(connection.state(), ConnectionState::Closed);
        assert!(!connection.permits_diagnostic_messages());
        assert_eq!(connection.tester_logical_address(), None);
    }
}
