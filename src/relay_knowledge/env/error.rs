//! Environment validation errors.

use std::{error::Error, fmt};

/// Environment parsing error with the exact variable that failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvError {
    pub variable: &'static str,
    pub kind: EnvErrorKind,
}

impl EnvError {
    pub(super) fn empty(variable: &'static str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::EmptyValue,
        }
    }

    pub(super) fn invalid_unicode(variable: &'static str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::InvalidUnicode,
        }
    }

    pub(super) fn invalid_integer(variable: &'static str, value: &str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::InvalidInteger {
                value: value.to_owned(),
            },
        }
    }

    pub(super) fn zero(variable: &'static str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::ZeroValue,
        }
    }

    pub(super) fn invalid_boolean(variable: &'static str, value: &str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::InvalidBoolean {
                value: value.to_owned(),
            },
        }
    }
}

/// Error category for environment parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvErrorKind {
    EmptyValue,
    InvalidUnicode,
    InvalidInteger { value: String },
    InvalidBoolean { value: String },
    ZeroValue,
}

impl fmt::Display for EnvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            EnvErrorKind::EmptyValue => write!(formatter, "{} must not be empty", self.variable),
            EnvErrorKind::InvalidUnicode => {
                write!(formatter, "{} must be valid UTF-8", self.variable)
            }
            EnvErrorKind::InvalidInteger { value } => {
                write!(
                    formatter,
                    "{} must be a positive integer, got '{value}'",
                    self.variable
                )
            }
            EnvErrorKind::InvalidBoolean { value } => write!(
                formatter,
                "{} must be true or false, got '{value}'",
                self.variable
            ),
            EnvErrorKind::ZeroValue => {
                write!(formatter, "{} must be greater than zero", self.variable)
            }
        }
    }
}

impl Error for EnvError {}
