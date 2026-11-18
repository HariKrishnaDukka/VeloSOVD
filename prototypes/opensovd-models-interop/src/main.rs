use opensovd_models::{
    UriReference,
    error::{ErrorCode, GenericError},
    version::{SovdInfo, VendorInfo, VersionInfo},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let error = GenericError::new(
        ErrorCode::IncompleteRequest,
        "A required diagnostic parameter is missing",
    );

    let version_info = VersionInfo {
        sovd_info: vec![SovdInfo {
            version: "1.1.0".to_owned(),
            base_uri: UriReference("http://127.0.0.1:8010/sovd/v1".to_owned()),
            vendor_info: Some(VendorInfo {
                version: "0.1.0".to_owned(),
                name: "VeloSOVD prototype".to_owned(),
            }),
        }],
        schema: None,
    };

    println!("Generic error:");
    println!("{}", serde_json::to_string_pretty(&error)?);
    println!("Version info:");
    println!("{}", serde_json::to_string_pretty(&version_info)?);

    Ok(())
}
