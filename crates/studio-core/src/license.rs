//! Licensing hook (F16 placeholder).
//!
//! Every licensable feature is declared here and checked through
//! [`feature_enabled`]. Until the licensing milestone all features return
//! `true`; F16 replaces the body with a signed, node-locked license check.
//! UI modules declare the feature they need, so licensing becomes
//! configuration, not a rewrite.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    Diagnostics,
    RawUds,
    OdxImport,
    Sequences,
    Reports,
    Flashing,
    AutomationApi,
}

pub const ALL: [Feature; 7] = [
    Feature::Diagnostics,
    Feature::RawUds,
    Feature::OdxImport,
    Feature::Sequences,
    Feature::Reports,
    Feature::Flashing,
    Feature::AutomationApi,
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LicenseStatus {
    pub licensed_to: String,
    pub kind: &'static str,
    pub features: Vec<(Feature, bool)>,
}

pub fn feature_enabled(_feature: Feature) -> bool {
    true // F16: replace with signed license verification
}

pub fn status() -> LicenseStatus {
    LicenseStatus {
        licensed_to: "development".into(),
        kind: "unlicensed-development",
        features: ALL.iter().map(|f| (*f, feature_enabled(*f))).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_features_enabled_until_licensing() {
        let s = status();
        assert_eq!(s.features.len(), ALL.len());
        assert!(s.features.iter().all(|(_, on)| *on));
        assert_eq!(
            serde_json::to_string(&Feature::RawUds).unwrap(),
            "\"raw-uds\""
        );
    }
}
