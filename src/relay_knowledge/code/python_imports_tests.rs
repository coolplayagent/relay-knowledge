use super::*;
#[test]
fn module_identity_is_separate_scoped_and_independent_of_path_order() {
    let a = PythonModuleOrigins::from_authorized_paths(["app.py", "typing.py"], &[], &[]);
    let b = PythonModuleOrigins::from_authorized_paths(["typing.py", "app.py"], &[], &[]);
    assert_eq!(a, b);
    assert!(!a.permits_standard_module("typing"));
    assert!(a.permits_standard_module("typing_extensions"));
    assert_eq!(
        PythonModuleOrigins::from_authorized_paths(["typing_extensions/__init__.py"], &[], &[])
            .typing_extensions,
        PythonModuleOrigin::Local
    );
    assert!(
        PythonModuleOrigins::from_authorized_paths(["unrelated/typing.py"], &[], &[])
            .permits_standard_module("typing")
    );
    assert!(
        !PythonModuleOrigins::from_authorized_paths(["app.py"], &["app.py".into()], &[])
            .permits_standard_module("typing")
    );
}
#[test]
fn incomplete_or_overbudget_inventory_does_not_prove_standard_origin() {
    assert!(!PythonModuleOrigins::default().permits_standard_module("typing"));
    let many = std::iter::repeat_n("x", 1_000_001);
    assert_eq!(
        PythonModuleOrigins::from_authorized_paths(many, &[], &[]),
        PythonModuleOrigins::default()
    );
    let long = "x".repeat(16 * 1024 * 1024 + 1);
    assert_eq!(
        PythonModuleOrigins::from_authorized_paths([long.as_str()], &[], &[]),
        PythonModuleOrigins::default()
    );
}
