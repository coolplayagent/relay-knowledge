//! Direct bounded-ranking contracts for exact import edges.

use super::scoring::{ImportSourceSignificance, import_source_significance_bonus};
use crate::domain::CodeQueryKind;

#[test]
fn substantive_production_importers_receive_a_bounded_tiebreaker() {
    let small = import_source_significance_bonus(
        4.0,
        "vendor.runtime.Client",
        &ImportSourceSignificance {
            path: "src/bootstrap.rs",
            is_generated: false,
            module: "import vendor.runtime.Client",
            target_hint: None,
            source_line_count: 24,
        },
        CodeQueryKind::Imports,
    );
    let substantive = import_source_significance_bonus(
        4.0,
        "vendor.runtime.Client",
        &ImportSourceSignificance {
            path: "src/client.rs",
            is_generated: false,
            module: "import vendor.runtime.Client",
            target_hint: None,
            source_line_count: 1_200,
        },
        CodeQueryKind::Imports,
    );
    let very_large = import_source_significance_bonus(
        4.0,
        "vendor.runtime.Client",
        &ImportSourceSignificance {
            path: "src/runtime.rs",
            is_generated: false,
            module: "import vendor.runtime.Client",
            target_hint: None,
            source_line_count: usize::MAX,
        },
        CodeQueryKind::Imports,
    );

    assert!(substantive > small);
    assert!(very_large <= 0.5);
}

#[test]
fn significance_does_not_promote_tests_or_nonmatching_edges() {
    assert_eq!(
        import_source_significance_bonus(
            4.0,
            "vendor.runtime.Client",
            &ImportSourceSignificance {
                path: "generated/client.rs",
                is_generated: true,
                module: "import vendor.runtime.Client",
                target_hint: None,
                source_line_count: 10_000,
            },
            CodeQueryKind::Imports,
        ),
        0.0
    );
    for wildcard in [
        "import * as Contexts from \"vendor.compiler\"",
        "use vendor::compiler::{*};",
    ] {
        assert_eq!(
            import_source_significance_bonus(
                4.0,
                "vendor.compiler",
                &ImportSourceSignificance {
                    path: "src/backend.rs",
                    is_generated: false,
                    module: wildcard,
                    target_hint: Some("vendor.compiler"),
                    source_line_count: 10_000,
                },
                CodeQueryKind::Imports,
            ),
            0.0,
            "{wildcard}"
        );
    }
    assert_eq!(
        import_source_significance_bonus(
            4.0,
            "vendor.compiler.Contexts.*",
            &ImportSourceSignificance {
                path: "src/backend.scala",
                is_generated: false,
                module: "import vendor.compiler.Contexts.*",
                target_hint: None,
                source_line_count: 10_000,
            },
            CodeQueryKind::Imports,
        ),
        0.0
    );
    assert_eq!(
        import_source_significance_bonus(
            4.0,
            "vendor.runtime.Client",
            &ImportSourceSignificance {
                path: "tests/client_test.rs",
                is_generated: false,
                module: "import vendor.runtime.Client",
                target_hint: None,
                source_line_count: 10_000,
            },
            CodeQueryKind::Imports,
        ),
        0.0
    );
    assert_eq!(
        import_source_significance_bonus(
            4.0,
            "vendor.runtime.Client",
            &ImportSourceSignificance {
                path: "src/server.rs",
                is_generated: false,
                module: "import vendor.runtime.Server",
                target_hint: None,
                source_line_count: 10_000,
            },
            CodeQueryKind::Imports,
        ),
        0.0
    );
    assert_eq!(
        import_source_significance_bonus(
            4.0,
            "vendor.runtime.Client",
            &ImportSourceSignificance {
                path: "src/client.rs",
                is_generated: false,
                module: "import vendor.runtime.Client",
                target_hint: None,
                source_line_count: 10_000,
            },
            CodeQueryKind::Hybrid,
        ),
        0.0
    );
}
