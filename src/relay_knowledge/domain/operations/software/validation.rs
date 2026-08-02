use super::super::{DomainError, error::required_text};

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
    let mut hash = 0xcbf29ce484222325u64;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("{prefix}:{hash:016x}")
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
