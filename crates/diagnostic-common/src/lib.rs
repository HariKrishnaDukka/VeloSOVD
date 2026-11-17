use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcuInfo {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticValue {
    pub name: String,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticFault {
    pub code: String,
    pub description: String,
    /// Indicates whether the UDS testFailed bit (bit 0) is set.
    pub active: bool,
    /// Original ISO 14229 DTC status byte.
    pub status: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    pub operation: String,
    pub success: bool,
    pub message: String,
}
