// crates/powerlink-rs-xdc/src/resolver/od.rs

use crate::error::XdcError;
use crate::model;
use crate::model::app_layers::DataTypeName;
use crate::parser::{parse_hex_u16, parse_hex_u8, parse_hex_string};
use crate::resolver::{utils, ValueMode};
use crate::types;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Iterates the `model::ObjectList` and resolves it into a rich, public `types::ObjectDictionary`.
pub(super) fn resolve_object_dictionary<'a>(
    app_layers: &'a model::app_layers::ApplicationLayers,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    type_map: &BTreeMap<String, DataTypeName>,
    mode: ValueMode,
) -> Result<types::ObjectDictionary, XdcError> {
    let mut od_objects = Vec::new();

    for model_obj in &app_layers.object_list.object {
        let index = parse_hex_u16(&model_obj.index)?;

        // --- Start: Resolve Object Attributes (Task 9) ---
        // Set defaults from the <Object> tag itself
        let mut resolved_access = model_obj.access_type.map(utils::map_access_type);
        let mut resolved_support = None;
        let mut resolved_persistent = false;
        let mut object_data: Option<Vec<u8>> = None;
        let mut od_sub_objects: Vec<types::SubObject> = Vec::new();

        // Check if a parameter reference overrides these attributes
        if let Some(id_ref) = model_obj.unique_id_ref.as_ref() {
            if let Some(param) = param_map.get(id_ref) {
                resolved_access = param.access.map(utils::map_param_access);
                resolved_support = param.support.map(utils::map_param_support);
                resolved_persistent = param.persistent;
            }
        }
        // --- End: Resolve Object Attributes ---

        if model_obj.object_type == "7" {
            // This is a VAR. Its value is on the <Object> element itself.
            let value_str_opt = get_value_str_for_object(model_obj, mode, param_map, template_map);

            // We only store data if it's valid hex.
            object_data = value_str_opt.and_then(|s| parse_hex_string(s).ok());

            // Perform type validation if we have data
            if let (Some(data), Some(data_type_id)) =
                (object_data.as_ref(), model_obj.data_type.as_deref())
            {
                utils::validate_type(index, 0, data, data_type_id, type_map)?;
            }
        } else {
            // This is a RECORD or ARRAY. Process its <SubObject> children.
            for model_sub_obj in &model_obj.sub_object {
                let sub_index = parse_hex_u8(&model_sub_obj.sub_index)?;

                // --- Start: Resolve SubObject Attributes (Task 9) ---
                let mut sub_resolved_access = model_sub_obj.access_type.map(utils::map_access_type);
                let mut sub_resolved_support = None;
                let mut sub_resolved_persistent = false;

                // Check if a parameter reference overrides these attributes
                if let Some(id_ref) = model_sub_obj.unique_id_ref.as_ref() {
                     if let Some(param) = param_map.get(id_ref) {
                        sub_resolved_access = param.access.map(utils::map_param_access);
                        sub_resolved_support = param.support.map(utils::map_param_support);
                        sub_resolved_persistent = param.persistent;
                    }
                }
                // --- End: Resolve SubObject Attributes ---

                // Logic to find the correct value string
                let value_str_opt = get_value_str_for_subobject(
                    model_sub_obj,
                    mode,
                    param_map,
                    template_map,
                    model_obj.unique_id_ref.as_ref(),
                    sub_index,
                );

                // We only store data if it's valid hex.
                // Non-hex values (like "NumberOfEntries") result in `None`.
                let data = value_str_opt.and_then(|s| parse_hex_string(s).ok());

                // Perform type validation if we have data
                if let (Some(data), Some(data_type_id)) = (
                    data.as_ref(),
                    model_sub_obj.data_type.as_deref(),
                ) {
                    utils::validate_type(index, sub_index, data, data_type_id, type_map)?;
                }
                
                let pdo_mapping = model_sub_obj.pdo_mapping.map(utils::map_pdo_mapping);

                od_sub_objects.push(types::SubObject {
                    sub_index,
                    name: model_sub_obj.name.clone(),
                    object_type: model_sub_obj.object_type.clone(),
                    data_type: model_sub_obj.data_type.clone(),
                    low_limit: model_sub_obj.low_limit.clone(),
                    high_limit: model_sub_obj.high_limit.clone(),
                    access_type: sub_resolved_access, // Use resolved value
                    pdo_mapping,
                    obj_flags: model_sub_obj.obj_flags.clone(),
                    support: sub_resolved_support, // Use resolved value
                    persistent: sub_resolved_persistent, // Use resolved value
                    data,
                });
            }
        }
        
        let pdo_mapping = model_obj.pdo_mapping.map(utils::map_pdo_mapping);

        od_objects.push(types::Object {
            index,
            name: model_obj.name.clone(),
            object_type: model_obj.object_type.clone(),
            data_type: model_obj.data_type.clone(),
            low_limit: model_obj.low_limit.clone(),
            high_limit: model_obj.high_limit.clone(),
            access_type: resolved_access, // Use resolved value
            pdo_mapping,
            obj_flags: model_obj.obj_flags.clone(),
            support: resolved_support, // Use resolved value
            persistent: resolved_persistent, // Use resolved value
            data: object_data,
            sub_objects: od_sub_objects,
        });
    }

    Ok(types::ObjectDictionary {
        objects: od_objects,
    })
}

/// Resolves the value string for an Object or Parameter.
/// (Helper for get_value_str_... functions)
fn resolve_value_from_param<'a>(
    param: &'a model::app_process::Parameter,
    mode: ValueMode,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
) -> Option<&'a String> {
    // 1. Check for a direct value on the parameter
    let direct_value = match mode {
        ValueMode::Actual => param.actual_value.as_ref().or(param.default_value.as_ref()),
        ValueMode::Default => param.default_value.as_ref().or(param.actual_value.as_ref()),
    };
    
    direct_value
        .map(|v| &v.value)
        .or_else(|| {
            // 2. If no direct value, check for a template reference
            param
                .template_id_ref
                .as_ref()
                .and_then(|template_id| template_map.get(template_id))
                .map(|v| &v.value)
        })
}

/// Helper to get the raw value string for a VAR object.
fn get_value_str_for_object<'a>(
    model_obj: &'a model::app_layers::Object,
    mode: ValueMode,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
) -> Option<&'a String> {
    // 1. Check for direct value on the <Object> tag
    let direct_value = match mode {
        ValueMode::Actual => model_obj.actual_value.as_ref().or(model_obj.default_value.as_ref()),
        ValueMode::Default => model_obj.default_value.as_ref().or(model_obj.actual_value.as_ref()),
    };

    direct_value.or_else(|| {
        // 2. If no direct value, resolve via uniqueIDRef
        model_obj
            .unique_id_ref
            .as_ref()
            .and_then(|id_ref| param_map.get(id_ref))
            .and_then(|param| resolve_value_from_param(param, mode, template_map))
    })
}

/// Helper to get the raw value string for a SubObject.
fn get_value_str_for_subobject<'a>(
    model_sub_obj: &'a model::app_layers::SubObject,
    mode: ValueMode,
    param_map: &'a BTreeMap<&'a String, &'a model::app_process::Parameter>,
    template_map: &'a BTreeMap<&'a String, &'a model::app_process::Value>,
    parent_unique_id_ref: Option<&'a String>,
    sub_index: u8,
) -> Option<&'a String> {
    // 1. Check for direct value on the <SubObject> tag
    let direct_value = match mode {
        ValueMode::Actual => model_sub_obj.actual_value.as_ref().or(model_sub_obj.default_value.as_ref()),
        ValueMode::Default => model_sub_obj.default_value.as_ref().or(model_sub_obj.actual_value.as_ref()),
    };
    
    direct_value
        .or_else(|| {
            // 2. If no direct value, resolve via SubObject's uniqueIDRef
            model_sub_obj
                .unique_id_ref
                .as_ref()
                .and_then(|id_ref| param_map.get(id_ref))
                .and_then(|param| resolve_value_from_param(param, mode, template_map))
        })
        .or_else(|| {
            // 3. If still None, and we are sub-index 0, check the parent Object's uniqueIDRef
            if sub_index == 0 {
                parent_unique_id_ref
                    .and_then(|id_ref| param_map.get(id_ref))
                    .and_then(|param| resolve_value_from_param(param, mode, template_map))
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::load_xdd_defaults_from_str;
    use crate::parser::load_xdc_from_str;
    use crate::types::{ParameterAccess, ParameterSupport};
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

    /// Test for Task 8 & 9: Verifies that attributes from `<parameter>`
    /// correctly override attributes from `<Object>`.
    #[test]
    fn test_resolve_unique_id_ref_attributes() {
        let app_layers_xml = r#"
        <ApplicationLayers>
          <ObjectList>
            <Object index="2000" name="Var1" objectType="7" dataType="0005"
                    accessType="ro" uniqueIDRef="param_1" />
          </ObjectList>
        </ApplicationLayers>"#;
        
        let app_process_xml = r#"
        <ApplicationProcess>
          <parameterList>
            <parameter uniqueID="param_1" access="readWrite" support="optional" persistent="true">
              <USINT />
            </parameter>
          </parameterList>
        </ApplicationProcess>"#;

        let xml = create_test_xml("", app_layers_xml, app_process_xml);
        let xdc_file = load_xdc_from_str(&xml).unwrap();

        let obj = &xdc_file.object_dictionary.objects[0];
        assert_eq!(obj.index, 0x2000);
        // Verify attributes were overridden by the <parameter>
        assert_eq!(obj.access_type, Some(ParameterAccess::ReadWrite));
        assert_eq!(obj.support, Some(ParameterSupport::Optional));
        assert_eq!(obj.persistent, true);
    }
    
    /// Test for Task 8 & 9: Verifies that attributes from `<Object>`
    /// are used when `uniqueIDRef` is absent.
    #[test]
    fn test_resolve_no_unique_id_ref() {
        let app_layers_xml = r#"
        <ApplicationLayers>
          <ObjectList>
            <Object index="2000" name="Var1" objectType="7" dataType="0005" accessType="const" />
          </ObjectList>
        </ApplicationLayers>"#;

        let xml = create_test_xml("", app_layers_xml, "");
        let xdc_file = load_xdc_from_str(&xml).unwrap();

        let obj = &xdc_file.object_dictionary.objects[0];
        assert_eq!(obj.index, 0x2000);
        // Verify attributes come from the <Object>
        assert_eq!(obj.access_type, Some(ParameterAccess::Constant));
        assert_eq!(obj.support, None);
        assert_eq!(obj.persistent, false);
    }

    /// Test for Task 8 & 9: Verifies value resolution logic for XDC (actualValue)
    /// vs. XDD (defaultValue) when using `uniqueIDRef`.
    #[test]
    fn test_resolve_unique_id_ref_value() {
        let app_layers_xml = r#"
        <ApplicationLayers>
          <ObjectList>
            <Object index="2000" name="Var1" objectType="7" dataType="0005"
                    actualValue="0x11" defaultValue="0x22" uniqueIDRef="param_1" />
          </ObjectList>
        </ApplicationLayers>"#;
        
        let app_process_xml = r#"
        <ApplicationProcess>
          <parameterList>
            <parameter uniqueID="param_1">
              <USINT />
              <actualValue value="0x88" />
              <defaultValue value="0x99" />
            </parameter>
          </parameterList>
        </ApplicationProcess>"#;

        let xml = create_test_xml("", app_layers_xml, app_process_xml);

        // 1. Test XDC loading (prioritizes `actualValue`)
        // The <Object> has `actualValue="0x11"`.
        // The <parameter> has `actualValue="0x88"`.
        // The <Object> `actualValue` should win.
        let xdc_file = load_xdc_from_str(&xml).unwrap();
        let xdc_obj = &xdc_file.object_dictionary.objects[0];
        assert_eq!(xdc_obj.data.as_deref(), Some(&[0x11_u8] as &[u8]));
        
        // 2. Test XDD loading (prioritizes `defaultValue`)
        // The <Object> has `defaultValue="0x22"`.
        // The <parameter> has `defaultValue="0x99"`.
        // The <Object> `defaultValue` should win.
        let xdd_file = load_xdd_defaults_from_str(&xml).unwrap();
        let xdd_obj = &xdd_file.object_dictionary.objects[0];
        assert_eq!(xdd_obj.data.as_deref(), Some(&[0x22_u8] as &[u8]));
    }
    
    /// Test for Task 8 & 9: Verifies value resolution fallback to `uniqueIDRef`
    /// when direct values are missing.
    #[test]
    fn test_resolve_unique_id_ref_value_fallback() {
        let app_layers_xml = r#"
        <ApplicationLayers>
          <ObjectList>
            <Object index="2000" name="Var1" objectType="7" dataType="0005"
                    uniqueIDRef="param_1" />
          </ObjectList>
        </ApplicationLayers>"#;
        
        let app_process_xml = r#"
        <ApplicationProcess>
          <parameterList>
            <parameter uniqueID="param_1">
              <USINT />
              <actualValue value="0x88" />
              <defaultValue value="0x99" />
            </parameter>
          </parameterList>
        </ApplicationProcess>"#;

        let xml = create_test_xml("", app_layers_xml, app_process_xml);

        // 1. Test XDC loading (no `actualValue` on <Object>, falls back to <parameter>)
        let xdc_file = load_xdc_from_str(&xml).unwrap();
        let xdc_obj = &xdc_file.object_dictionary.objects[0];
        assert_eq!(xdc_obj.data.as_deref(), Some(&[0x88_u8] as &[u8]));
        
        // 2. Test XDD loading (no `defaultValue` on <Object>, falls back to <parameter>)
        let xdd_file = load_xdd_defaults_from_str(&xml).unwrap();
        let xdd_obj = &xdd_file.object_dictionary.objects[0];
        assert_eq!(xdd_obj.data.as_deref(), Some(&[0x99_u8] as &[u8]));
    }
    
    /// Test for Task 8 & 9: Verifies value resolution from a `parameterTemplate`
    /// when the `parameter` itself has no value.
    #[test]
    fn test_resolve_template_id_ref_value() {
        let app_layers_xml = r#"
        <ApplicationLayers>
          <ObjectList>
            <Object index="2000" name="Var1" objectType="7" dataType="0005"
                    uniqueIDRef="param_1" />
          </ObjectList>
        </ApplicationLayers>"#;
        
        let app_process_xml = r#"
        <ApplicationProcess>
          <templateList>
            <parameterTemplate uniqueID="template_1">
              <USINT />
              <actualValue value="0xAA" />
              <defaultValue value="0xBB" />
            </parameterTemplate>
          </templateList>
          <parameterList>
            <parameter uniqueID="param_1" templateIDRef="template_1">
              <USINT />
            </parameter>
          </parameterList>
        </ApplicationProcess>"#;

        let xml = create_test_xml("", app_layers_xml, app_process_xml);

        // 1. Test XDC loading (falls back to template's `actualValue`)
        let xdc_file = load_xdc_from_str(&xml).unwrap();
        let xdc_obj = &xdc_file.object_dictionary.objects[0];
        assert_eq!(xdc_obj.data.as_deref(), Some(&[0xAA_u8] as &[u8]));
        
        // 2. Test XDD loading (falls back to template's `defaultValue`)
        let xdd_file = load_xdd_defaults_from_str(&xml).unwrap();
        let xdd_obj = &xdd_file.object_dictionary.objects[0];
        assert_eq!(xdd_obj.data.as_deref(), Some(&[0xBB_u8] as &[u8]));
    }
}