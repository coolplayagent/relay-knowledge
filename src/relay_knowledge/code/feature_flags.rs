use std::collections::BTreeMap;

use crate::domain::{CodeFeatureFlagRecord, DomainError, RepositoryCodeRange};

use super::stable_id;

mod comments;
mod config;
mod sources;

use comments::CommentState;
use config::{boolean_config_key, looks_like_config_file};
use sources::{config_read_keys, env_keys, preprocessor_flag_keys, usage_edge_kind};

pub(crate) struct FeatureFlagFileInput<'a> {
    pub(crate) repository_id: &'a str,
    pub(crate) source_scope: &'a str,
    pub(crate) file_id: &'a str,
    pub(crate) path: &'a str,
    pub(crate) language_id: &'a str,
    pub(crate) content: &'a str,
}

pub(crate) fn extract_feature_flags(
    input: FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut records = Vec::new();
    let mut byte_start = 0usize;
    let config_file = looks_like_config_file(input.path);
    let mut comment_state = CommentState::default();
    for (line_index, segment) in input.content.split_inclusive('\n').enumerate() {
        let line = segment.trim_end_matches('\n').trim_end_matches('\r');
        let Some(scan_line) = comment_state.scan_line(line, &input) else {
            byte_start = byte_start.saturating_add(segment.len());
            continue;
        };
        collect_line_records(
            &mut records,
            LineContext {
                input: &input,
                line,
                scan_line: &scan_line,
                line_number: line_index.saturating_add(1),
                byte_start,
                config_file,
            },
        )?;
        byte_start = byte_start.saturating_add(segment.len());
    }

    let mut deduped = BTreeMap::new();
    for record in records {
        deduped.insert(record.usage_id.clone(), record);
    }

    Ok(deduped.into_values().collect())
}

struct LineContext<'a, 'input> {
    input: &'a FeatureFlagFileInput<'input>,
    line: &'a str,
    scan_line: &'a str,
    line_number: usize,
    byte_start: usize,
    config_file: bool,
}

fn collect_line_records(
    records: &mut Vec<CodeFeatureFlagRecord>,
    context: LineContext<'_, '_>,
) -> Result<(), DomainError> {
    let mut line_records = Vec::new();
    for key in preprocessor_flag_keys(context.scan_line, context.input.language_id) {
        line_records.push(("preprocessor_symbol", key, "guards_code"));
    }
    for key in env_keys(context.scan_line) {
        line_records.push(("env_var", key, usage_edge_kind(context.scan_line)));
    }
    for key in config_read_keys(context.scan_line) {
        line_records.push(("config_key", key, usage_edge_kind(context.scan_line)));
    }
    if context.config_file
        && let Some(key) = boolean_config_key(context.scan_line)
    {
        line_records.push(("config_key", key, "defines_config"));
    }

    let mut seen = Vec::<(String, String, &'static str)>::new();
    for (source_kind, source_key, edge_kind) in line_records {
        if seen.iter().any(|(known_kind, known_key, known_edge)| {
            known_kind == source_kind && known_key == &source_key && known_edge == &edge_kind
        }) {
            continue;
        }
        seen.push((source_kind.to_owned(), source_key.clone(), edge_kind));
        records.push(feature_flag_record(
            &context,
            source_kind,
            &source_key,
            edge_kind,
        )?);
    }

    Ok(())
}

fn feature_flag_record(
    context: &LineContext<'_, '_>,
    source_kind: &str,
    source_key: &str,
    edge_kind: &str,
) -> Result<CodeFeatureFlagRecord, DomainError> {
    let input = context.input;
    let byte_end = context.byte_start.saturating_add(context.line.len());
    let byte_range =
        RepositoryCodeRange::new("feature_flag_byte_range", context.byte_start, byte_end)?;
    let line_range = RepositoryCodeRange::new(
        "feature_flag_line_range",
        context.line_number,
        context.line_number,
    )?;
    let feature_flag_id = stable_id(
        "feature_flag",
        [
            input.repository_id,
            input.source_scope,
            source_kind,
            source_key,
        ],
    );
    let usage_id = stable_id(
        "feature_flag_usage",
        [
            input.repository_id,
            input.source_scope,
            input.path,
            source_kind,
            source_key,
            edge_kind,
            &context.line_number.to_string(),
        ],
    );

    Ok(CodeFeatureFlagRecord {
        repository_id: input.repository_id.to_owned(),
        source_scope: input.source_scope.to_owned(),
        feature_flag_id,
        usage_id,
        file_id: input.file_id.to_owned(),
        path: input.path.to_owned(),
        language_id: input.language_id.to_owned(),
        name: feature_flag_name(source_key),
        source_kind: source_kind.to_owned(),
        source_key: source_key.to_owned(),
        edge_kind: edge_kind.to_owned(),
        confidence_basis_points: confidence_for_edge(edge_kind),
        confidence_tier: confidence_tier_for_edge(edge_kind).to_owned(),
        byte_range,
        line_range,
        excerpt: context.line.trim().to_owned(),
    })
}

fn feature_flag_name(source_key: &str) -> String {
    source_key
        .trim_matches(|character: char| !character.is_ascii_alphanumeric())
        .replace(['-', '.', ':'], "_")
        .to_ascii_lowercase()
}

fn confidence_for_edge(edge_kind: &str) -> u16 {
    match edge_kind {
        "defines_config" => 9_000,
        "guards_code" => 8_500,
        _ => 7_500,
    }
}

fn confidence_tier_for_edge(edge_kind: &str) -> &'static str {
    match edge_kind {
        "defines_config" | "guards_code" => "extracted",
        _ => "inferred",
    }
}

#[cfg(test)]
mod tests {
    use super::{FeatureFlagFileInput, env_keys, extract_feature_flags};

    fn input(content: &str) -> FeatureFlagFileInput<'_> {
        FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/lib.rs",
            language_id: "rust",
            content,
        }
    }

    #[test]
    fn extracts_full_file_flags_outside_symbol_chunks() {
        let records = extract_feature_flags(input(
            "const TOP_LEVEL: &str = \"x\";\nif std::env::var(\"CHECKOUT_V2\").is_ok() {}\n",
        ))
        .expect("feature flag records should extract");

        assert!(records.iter().any(|record| {
            record.source_key == "CHECKOUT_V2" && record.edge_kind == "guards_code"
        }));
    }

    #[test]
    fn captures_multiple_env_flags_on_one_line() {
        let records = extract_feature_flags(input(
            "if env::var(\"CHECKOUT_V2\").is_ok() && env::var(\"PAYMENTS_V2\").is_ok() {}\n",
        ))
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .any(|record| record.source_key == "CHECKOUT_V2")
        );
        assert!(
            records
                .iter()
                .any(|record| record.source_key == "PAYMENTS_V2")
        );
    }

    #[test]
    fn ignores_comment_only_feature_flag_examples() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.py",
            language_id: "python",
            content:
                "// std::env::var(\"COMMENTED_FLAG\")\n# config.get_bool(\"commented\")\nif config.get_bool(\"live\") {}\n",
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "live");
    }

    #[test]
    fn ignores_flags_inside_block_comments() {
        let records = extract_feature_flags(input(
            "/*\nstd::env::var(\"COMMENTED_FLAG\");\n*/\nif config.get_bool(\"live\") {}\n",
        ))
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "live");
    }

    #[test]
    fn ignores_inline_comment_feature_flag_examples() {
        let records = extract_feature_flags(input(
            "let _ = 1; // config.get_bool(\"commented\")\nlet _ = 2; /* std::env::var(\"BLOCKED\") */\nif config.get_bool(\"live\") {}\n",
        ))
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "live");
    }

    #[test]
    fn ignores_process_env_inside_string_literals() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.ts",
            language_id: "typescript",
            content:
                "console.log(\"use process.env.CHECKOUT_V2\");\nif (process.env.PAYMENTS_V2) {}\n",
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "PAYMENTS_V2");
    }

    #[test]
    fn extracts_flags_from_executable_star_prefixed_lines() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/lib.cc",
            language_id: "cpp",
            content: "*ptr = std::getenv(\"CHECKOUT_V2\");\n",
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "CHECKOUT_V2");
    }

    #[test]
    fn rust_lifetimes_do_not_hide_inline_comments() {
        let records = extract_feature_flags(input(
            "let marker: &'a str = value; // config.get_bool(\"commented\")\nif config.get_bool(\"live\") {}\n",
        ))
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "live");
    }

    #[test]
    fn keeps_nested_rust_block_comments_active() {
        let records = extract_feature_flags(input(
            "/*\n/*\nstd::env::var(\"INNER_COMMENT\")\n*/\nstd::env::var(\"OUTER_COMMENT\")\n*/\nif config.get_bool(\"live\") {}\n",
        ))
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "live");
    }

    #[test]
    fn keeps_nested_block_comments_active_for_nested_comment_languages() {
        for language_id in ["kotlin", "scala", "swift"] {
            let records = extract_feature_flags(FeatureFlagFileInput {
                repository_id: "repo",
                source_scope: "scope",
                file_id: "file",
                path: "src/app.code",
                language_id,
                content: "/*\n/*\nconfig.get_bool(\"inner\")\n*/\nconfig.get_bool(\"outer\")\n*/\nif config.get_bool(\"live\") {}\n",
            })
            .expect("feature flag records should extract");

            assert_eq!(records.len(), 1, "{language_id} should keep nesting");
            assert_eq!(records[0].source_key, "live");
        }
    }

    #[test]
    fn extracts_preprocessor_feature_gates() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/lib.c",
            language_id: "c",
            content: "#ifdef FEATURE_X\n#endif\n#if FEATURE_Y && defined(FEATURE_Z)\n#endif\n",
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 3);
        assert!(records.iter().any(|record| {
            record.source_kind == "preprocessor_symbol" && record.source_key == "FEATURE_X"
        }));
        assert!(
            records
                .iter()
                .any(|record| record.source_key == "FEATURE_Y")
        );
        assert!(
            records
                .iter()
                .any(|record| record.source_key == "FEATURE_Z")
        );
    }

    #[test]
    fn extracts_elif_and_all_preprocessor_expression_symbols() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/lib.c",
            language_id: "c",
            content: "#if FEATURE_A && FEATURE_B\n#elif FEATURE_C || defined(FEATURE_D)\n#endif\n",
        })
        .expect("feature flag records should extract");

        for source_key in ["FEATURE_A", "FEATURE_B", "FEATURE_C", "FEATURE_D"] {
            assert!(
                records.iter().any(|record| record.source_key == source_key),
                "{source_key} should be extracted"
            );
        }
    }

    #[test]
    fn treats_hash_lines_as_comments_in_config_files() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: ".env",
            language_id: "unknown",
            content: "# CHECKOUT_V2=true\nPAYMENTS_V2=true\n",
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "PAYMENTS_V2");
    }

    #[test]
    fn preserves_crlf_byte_offsets() {
        let records = extract_feature_flags(input(
            "let first = 1;\r\nif std::env::var(\"CHECKOUT_V2\").is_ok() {}\r\n",
        ))
        .expect("feature flag records should extract");
        let checkout = records
            .iter()
            .find(|record| record.source_key == "CHECKOUT_V2")
            .expect("checkout flag should exist");

        assert_eq!(checkout.byte_range.start, 16);
        assert_eq!(checkout.line_range.start, 2);
    }

    #[test]
    fn detects_common_source_key_shapes() {
        assert_eq!(
            env_keys("const enabled = process.env.CHECKOUT_V2 === '1';"),
            vec!["CHECKOUT_V2".to_owned()]
        );
        assert_eq!(
            env_keys("if process.env['CHECKOUT_V2'] && process.env.PAYMENTS_V2 {"),
            vec!["PAYMENTS_V2".to_owned(), "CHECKOUT_V2".to_owned()]
        );
    }
}
