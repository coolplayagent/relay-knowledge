// Direct tests for source-root stripping and module candidates.

use super::*;

#[test]
fn source_module_candidates_strip_nonstandard_roots() {
    assert!(
        source_module_candidates("external_deps/python_sdk/client.py")
            .contains(&"python_sdk/client.py".to_owned())
    );
    assert!(
        source_module_candidates("modules/java_sdk/src/main/java/example/Client.java")
            .contains(&"example/Client.java".to_owned())
    );
    assert!(
        source_module_candidates("lib/app/controller.rb").contains(&"app/controller.rb".to_owned())
    );
}

#[test]
fn source_module_candidates_do_not_strip_plain_vendor_or_third_party() {
    assert_eq!(
        source_module_candidates("vendor/pkg/foo.py"),
        vec!["vendor/pkg/foo.py".to_owned()]
    );
    assert_eq!(
        source_module_candidates("third_party/pkg/foo.py"),
        vec!["third_party/pkg/foo.py".to_owned()]
    );
}

#[test]
fn go_module_candidates_preserve_vendor_import_keys() {
    assert!(
        go_module_candidates("vendor/k8s.io/client-go/informers/factory.go")
            .contains(&"k8s.io/client-go/informers/factory.go".to_owned())
    );
}

#[test]
fn normalized_module_candidates_do_not_strip_import_specifier_roots() {
    assert_eq!(
        normalized_module_candidates("lib/foo.ts"),
        vec!["lib/foo.ts".to_owned()]
    );
    assert_eq!(
        normalized_module_candidates("./packages/foo.ts"),
        vec!["packages/foo.ts".to_owned()]
    );
}

#[test]
fn c_family_candidates_expose_include_roots() {
    assert!(
        c_family_module_candidates("external_deps/cpp_sdk/include/session_client.hpp")
            .contains(&"session_client.hpp".to_owned())
    );
}
