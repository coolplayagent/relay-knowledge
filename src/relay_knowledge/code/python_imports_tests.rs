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
fn unrelated_inventory_size_preserves_complete_module_evidence() {
    assert!(!PythonModuleOrigins::default().permits_standard_module("typing"));
    let many = std::iter::repeat_n("unrelated.py", 1_000_001);
    let origins = PythonModuleOrigins::from_authorized_paths(many, &[], &[]);
    assert!(origins.permits_standard_module("typing"));
    assert!(origins.permits_standard_module("typing_extensions"));
    let long = "x".repeat(16_385);
    let paths = std::iter::repeat_n(long.as_str(), 1024).chain(["typing.py"]);
    let origins = PythonModuleOrigins::from_authorized_paths(paths, &[], &[]);
    assert_eq!(origins.typing, PythonModuleOrigin::Local);
    assert!(origins.permits_standard_module("typing_extensions"));
    let paths = std::iter::repeat_n(long.as_str(), 1024);
    let restricted = PythonModuleOrigins::from_authorized_paths(paths, &["src".into()], &[]);
    assert_eq!(restricted, PythonModuleOrigins::default());
}
