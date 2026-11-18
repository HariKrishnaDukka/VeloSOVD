use opensovd_models::{
    JsonPointer, UriReference,
    error::{DataError, ErrorCode, ErrorDetails, GenericError},
    version::{SovdInfo, VendorInfo, VersionInfo},
};
use serde_json::json;

#[test]
fn generic_error_serializes_to_expected_sovd_shape_and_round_trips() {
    let error = GenericError::new(ErrorCode::IncompleteRequest, "Missing parameter");

    let encoded = serde_json::to_value(&error).expect("serialize generic error");
    assert_eq!(
        encoded,
        json!({
            "error_code": "incomplete-request",
            "message": "Missing parameter"
        })
    );

    let decoded: GenericError =
        serde_json::from_value(encoded.clone()).expect("deserialize generic error");
    let reencoded = serde_json::to_value(decoded).expect("re-serialize generic error");
    assert_eq!(reencoded, encoded);
}

#[test]
fn generic_error_omits_absent_optional_fields() {
    let error = GenericError::new(ErrorCode::SovdServerFailure, "Internal error");
    let encoded = serde_json::to_value(error).expect("serialize generic error");

    assert_eq!(
        encoded,
        json!({
            "error_code": "sovd-server-failure",
            "message": "Internal error"
        })
    );
    assert!(encoded.get("vendor_code").is_none());
    assert!(encoded.get("translation_id").is_none());
    assert!(encoded.get("parameters").is_none());
}

#[test]
fn data_error_with_json_pointer_round_trips() {
    let data_error = DataError {
        path: JsonPointer("/data/0/value".to_owned()),
        error: Some(GenericError::new(
            ErrorCode::InvalidResponseContent,
            "Invalid value",
        )),
    };
    let encoded = serde_json::to_value(&data_error).expect("serialize data error");

    assert_eq!(
        encoded,
        json!({
            "path": "/data/0/value",
            "error": {
                "error_code": "invalid-response-content",
                "message": "Invalid value"
            }
        })
    );

    let decoded: DataError =
        serde_json::from_value(encoded.clone()).expect("deserialize data error");
    assert_eq!(
        serde_json::to_value(decoded).expect("re-serialize data error"),
        encoded
    );
}

#[test]
fn untagged_error_details_accepts_generic_and_data_error_shapes() {
    let generic_json = json!({
        "error_code": "incomplete-request",
        "message": "Missing parameter"
    });
    let generic: ErrorDetails =
        serde_json::from_value(generic_json).expect("deserialize generic error details");
    assert!(matches!(generic, ErrorDetails::Generic(_)));

    let data_json = json!({ "path": "/data" });
    let data: ErrorDetails =
        serde_json::from_value(data_json).expect("deserialize data error details");
    assert!(matches!(data, ErrorDetails::Data(_)));
}

#[test]
fn version_info_omits_optional_schema_and_round_trips() {
    let response = VersionInfo {
        sovd_info: vec![SovdInfo {
            version: "1.1.0".to_owned(),
            base_uri: UriReference("https://diag.example.test/sovd/v1".to_owned()),
            vendor_info: Some(VendorInfo {
                version: "0.1.0".to_owned(),
                name: "VeloSOVD".to_owned(),
            }),
        }],
        schema: None,
    };

    let encoded = serde_json::to_value(&response).expect("serialize version info");
    assert_eq!(
        encoded,
        json!({
            "sovd_info": [{
                "version": "1.1.0",
                "base_uri": "https://diag.example.test/sovd/v1",
                "vendor_info": {
                    "version": "0.1.0",
                    "name": "VeloSOVD"
                }
            }]
        })
    );
    assert!(encoded.get("schema").is_none());

    let decoded: VersionInfo<VendorInfo> =
        serde_json::from_value(encoded.clone()).expect("deserialize version info");
    assert_eq!(
        serde_json::to_value(decoded).expect("re-serialize version info"),
        encoded
    );
}

#[test]
fn version_info_can_carry_optional_schema() {
    let response: VersionInfo<VendorInfo> = VersionInfo {
        sovd_info: vec![SovdInfo {
            version: "1.1.0".to_owned(),
            base_uri: UriReference("/sovd/v1".to_owned()),
            vendor_info: None,
        }],
        schema: Some(json!({ "type": "object" })),
    };

    let encoded = serde_json::to_value(response).expect("serialize version info with schema");
    assert_eq!(encoded["schema"], json!({ "type": "object" }));
}
