use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn designated_initializer_bonus_prefers_callback_table_entries() {
    let hybrid = request(
        "operation table read callback dispatch designated initializer",
        CodeQueryKind::Hybrid,
    );
    let bonus = designated_initializer_chunk_bonus(
        2.0,
        &hybrid.query,
        "const struct rk_driver_ops rk_default_ops = {\n\
                .open = rk_driver_open,\n\
                .read = rk_driver_read,\n\
                .close = rk_driver_close,\n\
            };",
        "src/driver_ops.c",
        &hybrid,
    );
    let declaration = designated_initializer_chunk_bonus(
        2.0,
        &hybrid.query,
        "struct rk_driver_ops {\n    rk_read_fn read;\n};",
        "include/driver_ops.h",
        &hybrid,
    );

    assert!(
        bonus > 3.0,
        "designated initializer bonus too small: {bonus}"
    );
    assert_eq!(declaration, 0.0);
}

#[test]
fn designated_initializer_bonus_prefers_multi_callable_tables() {
    let hybrid = request(
        "operation table read callback dispatch designated initializer",
        CodeQueryKind::Hybrid,
    );
    let sparse = designated_initializer_chunk_bonus(
        2.0,
        &hybrid.query,
        "static const struct rk_table_row rk_rows[] = {\n\
                [RK_STAGE_READ] = {\n\
                    .name = \"read\",\n\
                    .read = rk_driver_read,\n\
                },\n\
            };",
        "src/generated_table.c",
        &hybrid,
    );
    let multi_callable = designated_initializer_chunk_bonus(
        2.0,
        &hybrid.query,
        "const struct rk_driver_ops rk_default_ops = {\n\
                .open = rk_driver_open,\n\
                .read = rk_driver_read,\n\
                .close = rk_driver_close,\n\
            };",
        "src/driver_ops.c",
        &hybrid,
    );

    assert!(
        multi_callable > sparse,
        "sparse={sparse} multi={multi_callable}"
    );
}

#[test]
fn designated_initializer_bonus_detects_operation_surface_shorthand() {
    let hybrid = request(
        "operation table read callback dispatch designated initializer",
        CodeQueryKind::Hybrid,
    );
    let generic_table = designated_initializer_chunk_bonus(
        2.0,
        &hybrid.query,
        "const struct rk_driver_table rk_default_table = {\n\
                .open = rk_driver_open,\n\
                .read = rk_driver_read,\n\
                .close = rk_driver_close,\n\
            };",
        "src/driver_table.c",
        &hybrid,
    );
    let operation_table = designated_initializer_chunk_bonus(
        2.0,
        &hybrid.query,
        "const struct rk_driver_ops rk_default_ops = {\n\
                .open = rk_driver_open,\n\
                .read = rk_driver_read,\n\
                .close = rk_driver_close,\n\
            };",
        "src/driver_ops.c",
        &hybrid,
    );

    assert!(
        operation_table > generic_table + 0.5,
        "operation_table={operation_table} generic_table={generic_table}"
    );
}

#[test]
fn operation_surface_bonus_requires_multiple_callable_assignments() {
    let hybrid = request(
        "operation table read callback dispatch designated initializer",
        CodeQueryKind::Hybrid,
    );
    let sparse_operation_table = designated_initializer_chunk_bonus(
        2.0,
        &hybrid.query,
        "static const struct rk_driver_ops rk_default_ops = {\n\
                .name = \"read\",\n\
                .read = rk_driver_read,\n\
            };",
        "src/driver_ops.c",
        &hybrid,
    );

    assert!(
        sparse_operation_table < 5.0,
        "sparse operation table should not receive operation-surface bonus: {sparse_operation_table}"
    );
}

#[test]
fn designated_initializer_bonus_ignores_tests_without_test_intent() {
    let hybrid = request(
        "operation table read callback dispatch designated initializer",
        CodeQueryKind::Hybrid,
    );

    assert_eq!(
        designated_initializer_chunk_bonus(
            2.0,
            &hybrid.query,
            ".read = rk_driver_read,",
            "tests/fake_driver.c",
            &hybrid,
        ),
        0.0
    );
}

fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}
