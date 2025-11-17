// crates/powerlink-rs-xdc/src/resolver/header.rs

use crate::error::XdcError;
use crate::model;
use crate::types;

/// Parses a `model::ProfileHeader` into a `types::ProfileHeader`.
pub(super) fn resolve_header(
    model: &model::header::ProfileHeader,
) -> Result<types::ProfileHeader, XdcError> {
    Ok(types::ProfileHeader {
        identification: model.profile_identification.clone(),
        revision: model.profile_revision.clone(),
        name: model.profile_name.clone(),
        source: model.profile_source.clone(),
        date: model.profile_date.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model;
    use alloc::string::ToString;

    #[test]
    fn test_resolve_header() {
        // 1. Create the internal model header
        let model_header = model::header::ProfileHeader {
            profile_identification: "Test Profile ID".to_string(),
            profile_revision: "1.2.3".to_string(),
            profile_name: "MyTestDevice".to_string(),
            profile_source: "Test Source".to_string(),
            profile_date: Some("2025-01-01".to_string()),
            profile_class_id: model::header::ProfileClassId::Device,
            ..Default::default()
        };

        // 2. Call the resolver
        let result = resolve_header(&model_header);
        assert!(result.is_ok());
        let public_header = result.unwrap();

        // 3. Assert that all fields were mapped correctly
        assert_eq!(public_header.identification, "Test Profile ID");
        assert_eq!(public_header.revision, "1.2.3");
        assert_eq!(public_header.name, "MyTestDevice");
        assert_eq!(public_header.source, "Test Source");
        assert_eq!(public_header.date, Some("2025-01-01".to_string()));
    }
}
