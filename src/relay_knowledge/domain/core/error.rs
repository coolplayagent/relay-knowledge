use std::{error::Error, fmt};

/// Domain-level validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainError {
    pub field: &'static str,
    pub message: String,
}

impl DomainError {
    /// Builds a validation error with a stable field name.
    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.message)
    }
}

impl Error for DomainError {}

pub(crate) fn required_text(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, DomainError> {
    let text = value.into();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(DomainError::invalid(field, "must not be empty"));
    }

    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_field_and_message() {
        let error = DomainError::invalid("field", "failed");

        assert_eq!(error.to_string(), "field: failed");
    }
}
