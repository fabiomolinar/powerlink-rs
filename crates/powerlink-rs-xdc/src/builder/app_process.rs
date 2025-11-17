// crates/powerlink-rs-xdc/src/builder/app_process.rs

//! Contains builder functions to convert `types::ApplicationProcess` into `model::ApplicationProcess`.
// Note: This is a complex mapping and is simplified for XDC generation.
// It does not rebuild the *entire* ApplicationProcess, only what is needed for parameter context.
// For a full XDD generator, this would need to be much more extensive.

use crate::{model, types};

/// Converts a public `types::ApplicationProcess` into a `model::ApplicationProcess`.
pub(super) fn build_model_application_process(
    _public: &types::ApplicationProcess,
) -> model::app_process::ApplicationProcess {
    // For saving an XDC, the ApplicationProcess block is often simplified or
    // omitted, as all necessary data (defaults, access types) has already been
    // resolved and applied to the <ObjectList>.
    //
    // A full XDD *generator* would need to build this from scratch.
    // For XDC *serialization*, we only need to provide an empty <parameterList>
    // to satisfy the schema if the block is present.
    
    // We will build an empty ApplicationProcess block for now.
    // This can be expanded later if we need to write XDDs.
    model::app_process::ApplicationProcess {
        parameter_list: model::app_process::ParameterList {
            parameter: alloc::vec![], // Empty list
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types;
    use alloc::{string::ToString, vec};

    #[test]
    fn test_build_model_application_process() {
        // 1. Create a public ApplicationProcess (even with data)
        let public_app_proc = types::ApplicationProcess {
            data_types: vec![types::AppDataType::Struct(types::AppStruct {
                name: "MyStruct".to_string(),
                ..Default::default()
            })],
            ..Default::default()
        };

        // 2. Call the builder
        let model_app_proc = build_model_application_process(&public_app_proc);

        // 3. Verify
        // Our current builder implementation serializes an empty ApplicationProcess
        // for XDC files, as the data is already resolved into the ObjectList.
        // This test confirms that behavior.
        assert!(model_app_proc.data_type_list.is_none());
        assert!(model_app_proc.function_type_list.is_none());
        assert!(model_app_proc.function_instance_list.is_none());
        assert!(model_app_proc.template_list.is_none());
        assert!(model_app_proc.parameter_group_list.is_none());
        
        // It *must* contain a parameterList, even if empty.
        assert_eq!(model_app_proc.parameter_list.parameter.len(), 0);
    }
}