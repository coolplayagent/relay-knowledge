use super::super::{DomainError, error::required_text};
use crate::identity::StableHasher64;

pub(super) fn normalize_optional(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, DomainError> {
    value.map(|text| required_text(field, text)).transpose()
}

pub(super) fn validate_confidence(value: u16) -> Result<u16, DomainError> {
    if value > 10_000 {
        return Err(DomainError::invalid(
            "confidence",
            "must be between 0 and 10000 basis points",
        ));
    }

    Ok(value)
}

pub(super) fn stable_software_id<'a>(
    prefix: &str,
    parts: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut hasher = StableHasher64::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(&[0xff]);
    }

    format!("{prefix}:{:016x}", hasher.finish())
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
