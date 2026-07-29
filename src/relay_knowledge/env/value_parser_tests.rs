use std::ffi::OsString;

use super::{
    EnvErrorKind, RELAY_KNOWLEDGE_HOME, RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS, SSL_VERIFY,
    value_parser::{EnvironmentValues, bool_var, path_var, positive_usize_var},
};

#[test]
fn path_values_must_not_be_empty() {
    let values = EnvironmentValues::from([(OsString::from(RELAY_KNOWLEDGE_HOME), OsString::new())]);

    let error = path_var(&values, RELAY_KNOWLEDGE_HOME).expect_err("empty path should fail");

    assert_eq!(error.variable, RELAY_KNOWLEDGE_HOME);
    assert_eq!(error.kind, EnvErrorKind::EmptyValue);
}

#[test]
fn positive_integer_values_reject_invalid_and_zero_inputs() {
    let invalid = EnvironmentValues::from([(
        OsString::from(RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS),
        OsString::from("many"),
    )]);
    let error = positive_usize_var(&invalid, RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS)
        .expect_err("invalid integer should fail");
    assert_eq!(
        error.kind,
        EnvErrorKind::InvalidInteger {
            value: "many".to_owned()
        }
    );

    let zero = EnvironmentValues::from([(
        OsString::from(RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS),
        OsString::from("0"),
    )]);
    let error = positive_usize_var(&zero, RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS)
        .expect_err("zero should fail");
    assert_eq!(error.kind, EnvErrorKind::ZeroValue);
}

#[test]
fn boolean_values_report_invalid_text() {
    let values =
        EnvironmentValues::from([(OsString::from(SSL_VERIFY), OsString::from("sometimes"))]);

    let error = bool_var(&values, SSL_VERIFY).expect_err("invalid boolean should fail");

    assert_eq!(
        error.kind,
        EnvErrorKind::InvalidBoolean {
            value: "sometimes".to_owned()
        }
    );
}
