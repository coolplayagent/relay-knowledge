use super::super::{DomainError, error::required_text};

pub(crate) fn deserialize_language_filters<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = <Vec<String> as serde::Deserialize>::deserialize(deserializer)?;
    normalize_filter_list("language_filter", values).map_err(serde::de::Error::custom)
}

pub(crate) fn normalize_filter_list(
    field: &'static str,
    values: Vec<String>,
) -> Result<Vec<String>, DomainError> {
    if field == "language_filter"
        && (values.len() > 256 || values.iter().map(String::len).sum::<usize>() > 16_384)
    {
        return Err(DomainError::invalid(
            field,
            "language filter budget exceeded",
        ));
    }
    let mut normalized = Vec::new();
    for value in values {
        let value = required_text(field, value)?;
        if field == "language_filter"
            && (value.starts_with("__relay_") || value.contains([',', ';']))
        {
            return Err(DomainError::invalid(
                field,
                "internal language constraints are not request parameters",
            ));
        }
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }

    Ok(normalized)
}

pub(super) fn checked_u32(field: &'static str, value: usize) -> Result<u32, DomainError> {
    u32::try_from(value).map_err(|_| DomainError::invalid(field, "must fit in u32"))
}
