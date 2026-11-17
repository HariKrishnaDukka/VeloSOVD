// DoIP protocol implementation for the virtual SDV platform.

pub mod connection;
pub mod diagnostic;
pub mod header;
pub mod payload;
pub mod router;
pub mod routing;
pub mod transport;
pub mod vehicle;

// Diagnostic message payloads
pub use diagnostic::{DIAGNOSTIC_MESSAGE_MIN_LENGTH, DiagnosticMessage, DiagnosticPayloadError};

// Generic DoIP header
pub use header::{
    DEFAULT_MAX_PAYLOAD_LENGTH, DOIP_VERSION, DoipHeader, HEADER_LENGTH, HeaderError,
};

// DoIP payload types
pub use payload::DoipPayloadType;

// Vehicle identification and announcement payloads
pub use vehicle::{
    VehicleAnnouncement, VehicleIdentificationRequest, VehicleIdentificationRequestWithEid,
    VehicleIdentificationRequestWithVin, VehiclePayloadError,
};

// Routing activation payloads
pub use routing::{
    DEFAULT_DIAGNOSTIC_ACTIVATION_TYPE, ROUTING_RESPONSE_SUCCESS, RoutingActivationRequest,
    RoutingActivationResponse, RoutingPayloadError,
};

// Per-connection routing activation state
pub use connection::{ActivationError, ConnectionState, RoutingConnection};

// TCP frame transport
pub use transport::{DoipFrame, DoipTransport, TransportError};

// Diagnostic routing
pub use router::{
    DiagnosticRouter, RoutedDiagnosticResponse, RouterError, UdsDispatchFailure, UdsRequestHandler,
};
