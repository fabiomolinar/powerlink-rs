// crates/powerlink-rs-xdc/src/resolver/identity.rs

use crate::error::XdcError;
use crate::model;
use crate::model::common::{AttributedGlabels, Glabels, LabelChoice};
use crate::parser::parse_hex_u32;
use crate::types;
use alloc::string::String;

/// Helper to extract the first available `<label>` value from a `g_labels` group.
fn extract_label_from_glabels(labels: &Glabels) -> Option<String> {
    labels.items.iter().find_map(|item| {
        if let LabelChoice::Label(label) = item {
            Some(label.value.clone())
        } else {
            None
        }
    })
}

/// Helper to extract the first available `<label>` value from an `AttributedGlabels` struct.
fn extract_label_from_attributed_glabels(attributed_labels: &AttributedGlabels) -> Option<String> {
    extract_label_from_glabels(&attributed_labels.labels)
}

/// Parses a `model::DeviceIdentity` into a clean `types::Identity`.
pub(super) fn resolve_identity(model: &model::identity::DeviceIdentity) -> Result<types::Identity, XdcError> {
    let vendor_id = model
        .vendor_id
        .as_ref()
        .map(|v| parse_hex_u32(&v.value))
        .transpose()?
        .unwrap_or(0);

    // Try hex first, fall back to decimal if parsing fails (productID is often decimal)
    let product_id = model
        .product_id
        .as_ref()
        .map(|p| {
            parse_hex_u32(&p.value)
                .or_else(|_| p.value.parse::<u32>().map_err(|_| XdcError::InvalidAttributeFormat { attribute: "productID" } ))
                .ok()
        })
        .flatten()
        .unwrap_or(0);

    let versions = model
        .version
        .iter()
        .map(|v| types::Version {
            version_type: v.version_type.clone(),
            value: v.value.clone(),
        })
        .collect();
        
    let order_number = model
        .order_number
        .iter()
        .map(|on| on.value.clone())
        .collect();

    Ok(types::Identity {
        vendor_id,
        product_id,
        vendor_name: model.vendor_name.value.clone(),
        product_name: model.product_name.value.clone(),
        versions,
        
        // --- New fields ---
        vendor_text: model.vendor_text.as_ref().and_then(extract_label_from_attributed_glabels),
        device_family: model.device_family.as_ref().and_then(extract_label_from_attributed_glabels),
        product_family: model.product_family.as_ref().map(|pf| pf.value.clone()),
        product_text: model.product_text.as_ref().and_then(extract_label_from_attributed_glabels),
        order_number,
        build_date: model.build_date.clone(),
        specification_revision: model.specification_revision.as_ref().map(|sr| sr.value.clone()),
        instance_name: model.instance_name.as_ref().map(|i| i.value.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::common::{AttributedGlabels, Glabels, Label, LabelChoice, ReadOnlyString};
    use crate::model::identity::DeviceIdentity;
    use crate::parser::load_xdc_from_str;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::format;
    
    /// Creates a minimal, reusable XML string with a DeviceIdentity block.
    fn create_test_xml(device_identity: &str, app_layers: &str, app_process: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ISO15745ProfileContainer xmlns="http://www.ethernet-powerlink.org" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://www.ethernet-powerlink.org Powerlink_Main.xsd">
  <ISO15745Profile>
    <ProfileHeader>
      <ProfileIdentification>Test</ProfileIdentification>
      <ProfileRevision>1.0</ProfileRevision>
      <ProfileName>Test Profile</ProfileName>
      <ProfileSource>B&amp;R</ProfileSource>
      <ProfileClassID>Device</ProfileClassID>
      <ISO15745Reference>
        <ISO15745Part>4</ISO15745Part>
        <ISO15745Edition>1</ISO15745Edition>
        <ProfileTechnology>Powerlink</ProfileTechnology>
      </ISO15745Reference>
    </ProfileHeader>
    <ProfileBody xsi:type="ProfileBody_Device_Powerlink" fileName="test.xdd" fileCreator="B&amp;R" fileCreationDate="2024-01-01" fileVersion="1">
      {device_identity}
      {app_process}
    </ProfileBody>
  </ISO15745Profile>
  <ISO15745Profile>
    <ProfileHeader>
      <ProfileIdentification>Test</ProfileIdentification>
      <ProfileRevision>1.0</ProfileRevision>
      <ProfileName>Test Profile</ProfileName>
      <ProfileSource>B&amp;R</ProfileSource>
      <ProfileClassID>CommunicationNetwork</ProfileClassID>
      <ISO15745Reference>
        <ISO15745Part>4</ISO15745Part>
        <ISO15745Edition>1</ISO15745Edition>
        <ProfileTechnology>Powerlink</ProfileTechnology>
      </ISO15745Reference>
    </ProfileHeader>
    <ProfileBody xsi:type="ProfileBody_CommunicationNetwork_Powerlink" fileName="test.xdd" fileCreator="B&amp;R" fileCreationDate="2024-01-01" fileVersion="1">
      {app_layers}
      <NetworkManagement>
        <GeneralFeatures DLLFeatureMN="false" NMTBootTimeNotActive="0" NMTCycleTimeMax="0" NMTCycleTimeMin="0" NMTErrorEntries="0" />
      </NetworkManagement>
    </ProfileBody>
  </ISO15745Profile>
</ISO15745ProfileContainer>"#
        )
    }

    /// Test for Task 3 & 4: Verifies the new fields in `types::Identity` are populated.
    /// This is a good integration test for `resolve_identity`.
    #[test]
    fn test_resolve_identity() {
        let identity_xml = r#"
      <DeviceIdentity>
        <vendorName>B&amp;R</vendorName>
        <vendorID>0x0000001A</vendorID>
        <vendorText>
          <label lang="en">B&amp;R Industrial Automation</label>
        </vendorText>
        <deviceFamily>
          <label lang="en">X20 System</label>
        </deviceFamily>
        <productFamily readOnly="true">X20</productFamily>
        <productName readOnly="true">X20CP1584</productName>
        <productID readOnly="true">0x22B8</productID>
        <productText>
          <label lang="en">X20 CPU</label>
        </productText>
        <orderNumber readOnly="true">X20CP1584</orderNumber>
        <version versionType="HW" readOnly="true" value="1.0" />
        <buildDate>2024-01-01</buildDate>
        <specificationRevision readOnly="true">1.0.0</specificationRevision>
        <instanceName readOnly="false">MyCPU</instanceName>
      </DeviceIdentity>"#;

        let xml = create_test_xml(identity_xml, "<ApplicationLayers><ObjectList/></ApplicationLayers>", "");
        let xdc_file = load_xdc_from_str(&xml).unwrap();

        let id = &xdc_file.identity;
        assert_eq!(id.vendor_name, "B&R");
        assert_eq!(id.vendor_id, 0x1A);
        assert_eq!(id.product_name, "X20CP1584");
        assert_eq!(id.product_id, 0x22B8);
        assert_eq!(id.vendor_text, Some("B&R Industrial Automation".to_string()));
        assert_eq!(id.device_family, Some("X20 System".to_string()));
        assert_eq!(id.product_family, Some("X20".to_string()));
        assert_eq!(id.product_text, Some("X20 CPU".to_string()));
        assert_eq!(id.order_number, vec!["X20CP1584".to_string()]);
        assert_eq!(id.build_date, Some("2024-01-01".to_string()));
        assert_eq!(id.specification_revision, Some("1.0.0".to_string()));
        assert_eq!(id.instance_name, Some("MyCPU".to_string()));
        assert_eq!(id.versions[0].value, "1.0");
    }

    /// Unit test for `productID` parsing logic, checking hex and decimal.
    #[test]
    fn test_resolve_identity_product_id_parsing() {
        // 1. Test standard hex value
        let model_hex = DeviceIdentity {
            product_id: Some(ReadOnlyString { value: "0x1234".to_string(), ..Default::default() }),
            ..Default::default()
        };
        let identity_hex = resolve_identity(&model_hex).unwrap();
        assert_eq!(identity_hex.product_id, 0x1234);

        // 2. Test decimal value (common in some XDCs)
        let model_dec = DeviceIdentity {
            product_id: Some(ReadOnlyString { value: "1234".to_string(), ..Default::default() }),
            ..Default::default()
        };
        let identity_dec = resolve_identity(&model_dec).unwrap();
        assert_eq!(identity_dec.product_id, 1234);

        // 3. Test invalid value
        let model_invalid = DeviceIdentity {
            product_id: Some(ReadOnlyString { value: "not-a-number".to_string(), ..Default::default() }),
            ..Default::default()
        };
        let identity_invalid = resolve_identity(&model_invalid).unwrap();
        assert_eq!(identity_invalid.product_id, 0); // Should parse to 0 on failure
    }

    /// Unit test for the `extract_label_from_glabels` helper.
    #[test]
    fn test_extract_label_from_glabels() {
        // 1. Test empty
        let glabels_empty = Glabels { items: vec![] };
        assert_eq!(extract_label_from_glabels(&glabels_empty), None);

        // 2. Test only description
        let glabels_desc = Glabels {
            items: vec![LabelChoice::Description(model::common::Description {
                lang: "en".to_string(),
                value: "A description".to_string(),
                ..Default::default()
            })],
        };
        assert_eq!(extract_label_from_glabels(&glabels_desc), None);

        // 3. Test one label
        let glabels_one = Glabels {
            items: vec![LabelChoice::Label(Label {
                lang: "en".to_string(),
                value: "First Label".to_string(),
            })],
        };
        assert_eq!(extract_label_from_glabels(&glabels_one), Some("First Label".to_string()));

        // 4. Test multiple labels (should pick first)
        let glabels_multi = Glabels {
            items: vec![
                LabelChoice::Description(model::common::Description {
                    lang: "en".to_string(),
                    value: "A description".to_string(),
                    ..Default::default()
                }),
                LabelChoice::Label(Label {
                    lang: "en".to_string(),
                    value: "First Label".to_string(),
                }),
                LabelChoice::Label(Label {
                    lang: "de".to_string(),
                    value: "Zweite Beschriftung".to_string(),
                }),
            ],
        };
        assert_eq!(extract_label_from_glabels(&glabels_multi), Some("First Label".to_string()));
    }
    
    /// Unit test for the `extract_label_from_attributed_glabels` helper.
    #[test]
    fn test_extract_label_from_attributed_glabels() {
         let attributed = AttributedGlabels {
            labels: Glabels {
                items: vec![
                    LabelChoice::Description(model::common::Description {
                        lang: "en".to_string(),
                        value: "A description".to_string(),
                        ..Default::default()
                    }),
                    LabelChoice::Label(Label {
                        lang: "en".to_string(),
                        value: "The Label".to_string(),
                    }),
                ],
            },
            ..Default::default()
         };
         assert_eq!(extract_label_from_attributed_glabels(&attributed), Some("The Label".to_string()));

         let attributed_empty = AttributedGlabels::default();
         assert_eq!(extract_label_from_attributed_glabels(&attributed_empty), None);
    }
}