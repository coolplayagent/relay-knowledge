//! Typed extraction and validation for normalized environment snapshots.

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::PathBuf,
};

use super::EnvError;

pub(super) type EnvironmentValues = HashMap<OsString, OsString>;

pub(super) fn path_var(
    values: &EnvironmentValues,
    variable: &'static str,
) -> Result<Option<PathBuf>, EnvError> {
    values
        .get(OsStr::new(variable))
        .map(|value| {
            reject_empty(value, variable)?;
            Ok(PathBuf::from(value))
        })
        .transpose()
}

pub(super) fn first_path_var(
    values: &EnvironmentValues,
    variables: &[&'static str],
) -> Result<Option<PathBuf>, EnvError> {
    for variable in variables {
        if let Some(value) = path_var(values, variable)? {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

pub(super) fn string_var(
    values: &EnvironmentValues,
    variable: &'static str,
) -> Result<Option<String>, EnvError> {
    values
        .get(OsStr::new(variable))
        .map(|value| {
            reject_empty(value, variable)?;
            value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| EnvError::invalid_unicode(variable))
        })
        .transpose()
}

pub(super) fn first_string_var(
    values: &EnvironmentValues,
    variables: &[&'static str],
) -> Result<Option<String>, EnvError> {
    for variable in variables {
        if let Some(value) = string_var(values, variable)? {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

pub(super) fn bool_var(
    values: &EnvironmentValues,
    variable: &'static str,
) -> Result<Option<bool>, EnvError> {
    string_var(values, variable)?
        .map(|value| parse_bool(variable, &value))
        .transpose()
}

pub(super) fn first_bool_var(
    values: &EnvironmentValues,
    variables: &[&'static str],
) -> Result<Option<bool>, EnvError> {
    for variable in variables {
        if let Some(value) = bool_var(values, variable)? {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

pub(super) fn positive_u64_var(
    values: &EnvironmentValues,
    variable: &'static str,
) -> Result<Option<u64>, EnvError> {
    string_var(values, variable)?
        .map(|value| parse_positive_u64(variable, &value))
        .transpose()
}

pub(super) fn positive_usize_var(
    values: &EnvironmentValues,
    variable: &'static str,
) -> Result<Option<usize>, EnvError> {
    string_var(values, variable)?
        .map(|value| parse_positive_usize(variable, &value))
        .transpose()
}

fn parse_positive_u64(variable: &'static str, value: &str) -> Result<u64, EnvError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| EnvError::invalid_integer(variable, value))?;

    if parsed == 0 {
        return Err(EnvError::zero(variable));
    }

    Ok(parsed)
}

fn parse_positive_usize(variable: &'static str, value: &str) -> Result<usize, EnvError> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| EnvError::invalid_integer(variable, value))?;

    if parsed == 0 {
        return Err(EnvError::zero(variable));
    }

    Ok(parsed)
}

fn parse_bool(variable: &'static str, value: &str) -> Result<bool, EnvError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(EnvError::invalid_boolean(variable, value)),
    }
}

fn reject_empty(value: &OsString, variable: &'static str) -> Result<(), EnvError> {
    if value.is_empty() {
        return Err(EnvError::empty(variable));
    }

    Ok(())
}

#[cfg(test)]
#[path = "value_parser_tests.rs"]
mod value_parser_tests;
