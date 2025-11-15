// crates/powerlink-rs-xdc/src/resolver/header.rs

use crate::error::XdcError;
use crate::model;
use crate::types;

/// Parses a `model::ProfileHeader` into a `types::ProfileHeader`.
pub(super) fn resolve_header(model: &model::header::ProfileHeader) -> Result<types::ProfileHeader, XdcError> {
    Ok(types::ProfileHeader {
        identification: model.profile_identification.clone(),
        revision: model.profile_revision.clone(),
        name: model.profile_name.clone(),
        source: model.profile_source.clone(),
        date: model.profile_date.clone(),
    })
}