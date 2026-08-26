use super::{QualityGate, QualityGateStage};
use crate::config::ProductBinaryProfile;

const BM25_HIERARCHY_BUILD_TIMEOUT_SECONDS: u64 = 1200;
const BM25_HIERARCHY_SUITE_TIMEOUT_SECONDS: u64 = 120;
const CODE_INDEX_PERSISTENCE_PERFORMANCE_TIMEOUT_SECONDS: u64 = 120;

// These gates must remain consecutive singleton stages: the first owns cold
// compilation and the second measures the named suite without a competing
// Cargo process or inherited compile/link time.

pub(super) fn quality_gate_stages(
    profile: &str,
    product_binary_profile: Option<ProductBinaryProfile>,
) -> Vec<QualityGateStage> {
    if profile == "smoke" {
        return vec![QualityGateStage::Parallel(vec![
            quality_gate(
                "cargo_fmt_check",
                ["cargo", "fmt", "--all", "--", "--check"],
                120,
            ),
            quality_gate(
                "self_iteration_cargo_fmt_check",
                [
                    "cargo",
                    "fmt",
                    "--manifest-path",
                    "tools/self_iteration/Cargo.toml",
                    "--",
                    "--check",
                ],
                120,
            ),
        ])];
    }
    if profile == "fast" {
        return vec![
            QualityGateStage::Parallel(vec![
                quality_gate(
                    "cargo_fmt_check",
                    ["cargo", "fmt", "--all", "--", "--check"],
                    120,
                ),
                quality_gate(
                    "self_iteration_cargo_fmt_check",
                    [
                        "cargo",
                        "fmt",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--",
                        "--check",
                    ],
                    120,
                ),
                linux_glibc_compatibility_policy_gate(),
                skill_metadata_policy_gate(),
            ]),
            QualityGateStage::Parallel(vec![product_binary_build_gate(
                product_binary_profile.expect("non-smoke profile must select a product binary"),
            )]),
            QualityGateStage::Parallel(vec![bm25_hierarchy_build_gate()]),
            QualityGateStage::Parallel(vec![bm25_hierarchy_gate()]),
            QualityGateStage::Parallel(vec![code_index_persistence_performance_gate()]),
            QualityGateStage::Parallel(vec![
                quality_gate(
                    "self_iteration_cargo_check",
                    [
                        "cargo",
                        "check",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--all-targets",
                    ],
                    180,
                ),
                quality_gate(
                    "code_index_recovery_cases",
                    ["cargo", "test", "--all-targets", "code_index_task_"],
                    300,
                ),
                quality_gate(
                    "business_knowledge_regression_cases",
                    ["cargo", "test", "--lib", "business", "--", "--nocapture"],
                    180,
                ),
                quality_gate(
                    "code_index_sqlite_lock_cases",
                    [
                        "cargo",
                        "test",
                        "--all-targets",
                        "code_index_sqlite_lock_cases",
                    ],
                    300,
                ),
                quality_gate(
                    "code_index_health_isolation_cases",
                    [
                        "cargo",
                        "test",
                        "--test",
                        "relay_knowledge",
                        "code_index_health_isolation_cases",
                        "--",
                        "--nocapture",
                    ],
                    300,
                ),
            ]),
        ];
    }
    vec![
        QualityGateStage::Parallel(vec![
            quality_gate(
                "cargo_fmt_check",
                ["cargo", "fmt", "--all", "--", "--check"],
                120,
            ),
            quality_gate(
                "self_iteration_cargo_fmt_check",
                [
                    "cargo",
                    "fmt",
                    "--manifest-path",
                    "tools/self_iteration/Cargo.toml",
                    "--",
                    "--check",
                ],
                120,
            ),
            linux_glibc_compatibility_policy_gate(),
        ]),
        QualityGateStage::Parallel(vec![
            product_binary_build_gate(
                product_binary_profile.expect("non-smoke profile must select a product binary"),
            ),
            quality_gate(
                "self_iteration_cargo_build_release",
                [
                    "cargo",
                    "build",
                    "--release",
                    "--manifest-path",
                    "tools/self_iteration/Cargo.toml",
                    "--bin",
                    "relay-knowledge-self-iterate",
                ],
                300,
            ),
        ]),
        QualityGateStage::Parallel(vec![bm25_hierarchy_build_gate()]),
        QualityGateStage::Parallel(vec![bm25_hierarchy_gate()]),
        QualityGateStage::Rails(vec![
            vec![
                quality_gate(
                    "cargo_clippy",
                    [
                        "cargo",
                        "clippy",
                        "--all-targets",
                        "--all-features",
                        "--",
                        "-D",
                        "warnings",
                    ],
                    1200,
                ),
                quality_gate(
                    "cargo_test",
                    ["cargo", "test", "--all-targets", "--all-features"],
                    1200,
                ),
            ],
            vec![
                quality_gate(
                    "self_iteration_cargo_clippy",
                    [
                        "cargo",
                        "clippy",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--all-targets",
                        "--",
                        "-D",
                        "warnings",
                    ],
                    300,
                ),
                quality_gate(
                    "self_iteration_cargo_test",
                    [
                        "cargo",
                        "test",
                        "--manifest-path",
                        "tools/self_iteration/Cargo.toml",
                        "--all-targets",
                    ],
                    300,
                ),
            ],
        ]),
    ]
}

fn product_binary_build_gate(profile: ProductBinaryProfile) -> QualityGate {
    match profile {
        ProductBinaryProfile::Debug => quality_gate(
            "cargo_build_debug",
            ["cargo", "build", "--bin", "relay-knowledge"],
            600,
        ),
        ProductBinaryProfile::Release => quality_gate(
            "cargo_build_release",
            ["cargo", "build", "--release", "--bin", "relay-knowledge"],
            1200,
        ),
    }
}

fn linux_glibc_compatibility_policy_gate() -> QualityGate {
    quality_gate(
        "linux_glibc_compatibility_policy",
        [
            "python3",
            "tools/release/check_linux_glibc_compat.py",
            "--self-test",
            "--verify-workflow",
            ".github/workflows/release.yml",
        ],
        60,
    )
}

fn skill_metadata_policy_gate() -> QualityGate {
    quality_gate(
        "skill_metadata_policy_cases",
        [
            "bash",
            "-lc",
            "set -euo pipefail\nmanifest_version=\"$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)[\"packages\"][0][\"version\"])')\"\npython3 tools/release/update_skill_metadata_version.py --self-test --check skills/relay-knowledge-cli/SKILL.md \"$manifest_version\"",
        ],
        60,
    )
}

fn bm25_hierarchy_build_gate() -> QualityGate {
    quality_gate(
        "bm25_hierarchy_build",
        ["cargo", "test", "--lib", "--all-features", "--no-run"],
        BM25_HIERARCHY_BUILD_TIMEOUT_SECONDS,
    )
}

fn bm25_hierarchy_gate() -> QualityGate {
    quality_gate(
        "bm25_hierarchy_suite",
        [
            "cargo",
            "test",
            "--lib",
            "--all-features",
            "bm25_hierarchy_suite",
            "--",
            "--nocapture",
        ],
        BM25_HIERARCHY_SUITE_TIMEOUT_SECONDS,
    )
}

fn code_index_persistence_performance_gate() -> QualityGate {
    quality_gate(
        "code_index_persistence_performance_suite",
        [
            "cargo",
            "test",
            "--lib",
            "--all-features",
            "code_index_persistence_performance_suite",
            "--",
            "--nocapture",
        ],
        CODE_INDEX_PERSISTENCE_PERFORMANCE_TIMEOUT_SECONDS,
    )
}

fn quality_gate<const N: usize>(
    name: &'static str,
    command: [&'static str; N],
    timeout_seconds: u64,
) -> QualityGate {
    QualityGate {
        name,
        command: command.into_iter().map(ToOwned::to_owned).collect(),
        timeout_seconds,
    }
}

pub(super) fn quality_budget_ms(name: &str) -> Option<f64> {
    match name {
        "cargo_build_debug" => Some(90_000.0),
        "code_index_recovery_cases" => Some(60_000.0),
        "business_knowledge_regression_cases" => Some(30_000.0),
        "code_index_sqlite_lock_cases" => Some(60_000.0),
        "bm25_hierarchy_suite" => Some(30_000.0),
        "code_index_persistence_performance_suite" => Some(30_000.0),
        "self_iteration_cargo_check" => Some(30_000.0),
        "cargo_build_release" => Some(180_000.0),
        "self_iteration_cargo_build_release" => Some(60_000.0),
        "cargo_fmt_check" => Some(20_000.0),
        "self_iteration_cargo_fmt_check" => Some(20_000.0),
        "linux_glibc_compatibility_policy" => Some(10_000.0),
        "skill_metadata_policy_cases" => Some(10_000.0),
        "cargo_clippy" => Some(180_000.0),
        "self_iteration_cargo_clippy" => Some(60_000.0),
        "cargo_test" => Some(240_000.0),
        "self_iteration_cargo_test" => Some(60_000.0),
        _ => None,
    }
}

#[cfg(test)]
#[path = "gate_policy_tests.rs"]
mod tests;
