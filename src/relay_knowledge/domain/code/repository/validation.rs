use super::super::{DomainError, error::required_text};

pub(super) fn normalize_filter_list(
    field: &'static str,
    values: Vec<String>,
) -> Result<Vec<String>, DomainError> {
    let mut normalized = Vec::new();
    for value in values {
        let value = required_text(field, value)?;
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }

    Ok(normalized)
}

pub(super) fn checked_u32(field: &'static str, value: usize) -> Result<u32, DomainError> {
    u32::try_from(value).map_err(|_| DomainError::invalid(field, "must fit in u32"))
}
