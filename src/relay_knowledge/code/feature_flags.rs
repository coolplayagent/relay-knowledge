use std::collections::BTreeMap;

use crate::domain::{CodeFeatureFlagRecord, DomainError, RepositoryCodeRange};

use super::{
    configuration::{ConfigFact, ConfigRange, ConfigValueKind},
    stable_id,
};

mod comments;
mod config;
mod sources;

use comments::CommentState;
use config::{boolean_config_keys, looks_like_config_file};
use sources::{
    ParameterBodyStatus, config_read_keys, env_keys, function_parameter_body_status,
    function_parameter_receivers, preprocessor_flag_keys, sdk_continued_flag_key,
    sdk_flag_keys_for_line, sdk_pending_argument_index, usage_edge_kind,
};

pub(crate) struct FeatureFlagFileInput<'a> {
    pub(crate) repository_id: &'a str,
    pub(crate) source_scope: &'a str,
    pub(crate) file_id: &'a str,
    pub(crate) path: &'a str,
    pub(crate) language_id: &'a str,
    pub(crate) content: &'a str,
    pub(crate) config_facts: &'a [ConfigFact],
}

pub(crate) fn extract_feature_flags(
    input: FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut records = Vec::new();
    let mut byte_start = 0usize;
    let config_file = looks_like_config_file(input.path);
    let mut comment_state = CommentState::default();
    let mut sdk_receivers = BTreeMap::new();
    let mut shadowed_sdk_receivers = BTreeMap::new();
    let mut pending_shadowed_sdk_receivers = BTreeMap::new();
    let mut shadowed_sdk_depth: Option<usize> = None;
    let mut pending_sdk_call: Option<PendingSdkCall> = None;
    let mut brace_depth = 0usize;
    for (line_index, segment) in input.content.split_inclusive('\n').enumerate() {
        let line = segment.trim_end_matches('\n').trim_end_matches('\r');
        let Some(scan_line) = comment_state.scan_line(line, &input) else {
            byte_start = byte_start.saturating_add(segment.len());
            continue;
        };
        start_pending_shadow_scope(
            &scan_line,
            &mut pending_shadowed_sdk_receivers,
            &mut shadowed_sdk_receivers,
            &mut shadowed_sdk_depth,
        );
        let mut continued_sdk_key = None;
        if let Some(pending) = pending_sdk_call.take() {
            if let Some(key) = sdk_continued_flag_key(&scan_line, pending.argument_index) {
                continued_sdk_key = Some((key, pending.edge_kind));
            } else if let Some(argument_index) =
                sources::sdk_next_pending_argument_index(&scan_line, pending.argument_index)
            {
                pending_sdk_call = Some(PendingSdkCall {
                    argument_index,
                    edge_kind: pending.edge_kind,
                });
            }
        }
        let shadowed_on_line = function_parameter_receivers(&scan_line, input.language_id);
        let body_status = function_parameter_body_status(&scan_line, input.language_id);
        let (brace_opens, brace_closes) = brace_counts(&scan_line);
        let starts_shadow_scope = body_status == ParameterBodyStatus::Block;
        let pending_shadow_scope = body_status == ParameterBodyStatus::Pending;
        if starts_shadow_scope && shadowed_sdk_depth.is_none() {
            shadowed_sdk_depth = Some(0);
        }
        let mut line_shadowed_receivers = BTreeMap::new();
        for receiver in shadowed_on_line {
            if let Some(scope_depth) = sdk_receivers.remove(&receiver) {
                if starts_shadow_scope {
                    shadowed_sdk_receivers.insert(receiver, scope_depth);
                } else if pending_shadow_scope {
                    pending_shadowed_sdk_receivers.insert(receiver, scope_depth);
                } else {
                    line_shadowed_receivers.insert(receiver, scope_depth);
                }
            }
        }
        let sdk_keys = sdk_flag_keys_for_line(&scan_line, &mut sdk_receivers, brace_depth);
        collect_line_records(
            &mut records,
            LineContext {
                input: &input,
                line,
                scan_line: &scan_line,
                line_number: line_index.saturating_add(1),
                byte_start,
                config_file,
                continued_sdk_key,
                sdk_keys,
            },
        )?;
        if pending_sdk_call.is_none() {
            if let Some(argument_index) = sdk_pending_argument_index(&scan_line, &sdk_receivers) {
                pending_sdk_call = Some(PendingSdkCall {
                    argument_index,
                    edge_kind: usage_edge_kind(&scan_line),
                });
            }
        }
        update_sdk_shadow_scope(
            (brace_opens, brace_closes),
            &mut shadowed_sdk_depth,
            &mut shadowed_sdk_receivers,
            &mut sdk_receivers,
        );
        for receiver in line_shadowed_receivers {
            sdk_receivers.insert(receiver.0, receiver.1);
        }
        brace_depth = brace_depth
            .saturating_add(brace_opens)
            .saturating_sub(brace_closes);
        expire_scoped_sdk_receivers(&mut sdk_receivers, brace_depth);
        byte_start = byte_start.saturating_add(segment.len());
    }
    collect_config_fact_records(&mut records, &input)?;

    let mut deduped = BTreeMap::new();
    for record in records {
        deduped.insert(record.usage_id.clone(), record);
    }

    Ok(deduped.into_values().collect())
}

fn collect_config_fact_records(
    records: &mut Vec<CodeFeatureFlagRecord>,
    input: &FeatureFlagFileInput<'_>,
) -> Result<(), DomainError> {
    for fact in input.config_facts {
        if fact.kind != "config_key" || fact.value_kind != ConfigValueKind::Boolean {
            continue;
        }
        records.push(feature_flag_record_from_range(
            input,
            "config_key",
            &fact.name,
            "defines_config",
            fact.range,
            &excerpt_for_range(input.content, fact.range),
        )?);
    }

    Ok(())
}

fn update_sdk_shadow_scope(
    braces: (usize, usize),
    shadow_depth: &mut Option<usize>,
    shadowed_receivers: &mut BTreeMap<String, usize>,
    sdk_receivers: &mut BTreeMap<String, usize>,
) {
    let Some(current_depth) = shadow_depth.as_mut() else {
        return;
    };
    let (opens, closes) = braces;
    *current_depth = current_depth.saturating_add(opens).saturating_sub(closes);
    if *current_depth == 0 && (opens > 0 || closes > 0) {
        for (receiver, scope_depth) in std::mem::take(shadowed_receivers) {
            sdk_receivers.insert(receiver, scope_depth);
        }
        *shadow_depth = None;
    }
}

fn start_pending_shadow_scope(
    line: &str,
    pending_receivers: &mut BTreeMap<String, usize>,
    shadowed_receivers: &mut BTreeMap<String, usize>,
    shadow_depth: &mut Option<usize>,
) {
    if pending_receivers.is_empty() || !line.trim_start().starts_with('{') {
        return;
    }
    if shadow_depth.is_none() {
        *shadow_depth = Some(0);
    }
    shadowed_receivers.append(pending_receivers);
}

fn expire_scoped_sdk_receivers(sdk_receivers: &mut BTreeMap<String, usize>, brace_depth: usize) {
    sdk_receivers.retain(|_, scope_depth| *scope_depth <= brace_depth);
}

fn brace_counts(line: &str) -> (usize, usize) {
    let mut opens = 0usize;
    let mut closes = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'' | '`') {
            quote = Some(character);
            continue;
        }
        if character == '{' {
            opens = opens.saturating_add(1);
        } else if character == '}' {
            closes = closes.saturating_add(1);
        }
    }

    (opens, closes)
}

struct PendingSdkCall {
    argument_index: usize,
    edge_kind: &'static str,
}

struct LineContext<'a, 'input> {
    input: &'a FeatureFlagFileInput<'input>,
    line: &'a str,
    scan_line: &'a str,
    line_number: usize,
    byte_start: usize,
    config_file: bool,
    continued_sdk_key: Option<(String, &'static str)>,
    sdk_keys: Vec<String>,
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
    for key in &context.sdk_keys {
        line_records.push((
            "sdk_flag_key",
            key.clone(),
            usage_edge_kind(context.scan_line),
        ));
    }
    if let Some((key, edge_kind)) = &context.continued_sdk_key {
        line_records.push(("sdk_flag_key", key.clone(), *edge_kind));
    }
    if context.config_file {
        for key in boolean_config_keys(context.scan_line) {
            line_records.push(("config_key", key, "defines_config"));
        }
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
    feature_flag_record_from_range(
        input,
        source_kind,
        source_key,
        edge_kind,
        ConfigRange {
            byte_start: context.byte_start,
            byte_end,
            line_start: context.line_number,
            line_end: context.line_number,
        },
        context.line.trim(),
    )
}

fn feature_flag_record_from_range(
    input: &FeatureFlagFileInput<'_>,
    source_kind: &str,
    source_key: &str,
    edge_kind: &str,
    range: ConfigRange,
    excerpt: &str,
) -> Result<CodeFeatureFlagRecord, DomainError> {
    let byte_range =
        RepositoryCodeRange::new("feature_flag_byte_range", range.byte_start, range.byte_end)?;
    let line_range =
        RepositoryCodeRange::new("feature_flag_line_range", range.line_start, range.line_end)?;
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
            &range.line_start.to_string(),
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
        excerpt: excerpt.to_owned(),
    })
}

fn excerpt_for_range(content: &str, range: ConfigRange) -> String {
    if let Some(source) = content.get(range.byte_start..range.byte_end) {
        return source.trim().to_owned();
    }

    content
        .lines()
        .nth(range.line_start.saturating_sub(1))
        .unwrap_or_default()
        .trim()
        .to_owned()
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
    use crate::code::configuration::{ConfigFact, ConfigRange, ConfigValueKind};

    fn input(content: &str) -> FeatureFlagFileInput<'_> {
        FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/lib.rs",
            language_id: "rust",
            content,
            config_facts: &[],
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
    fn emits_definitions_from_structured_configuration_facts() {
        let config_facts = [ConfigFact {
            name: "checkout_v2".to_owned(),
            kind: "config_key",
            value_kind: ConfigValueKind::Boolean,
            range: ConfigRange {
                byte_start: 0,
                byte_end: 17,
                line_start: 1,
                line_end: 1,
            },
        }];
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "docs/flags.txt",
            language_id: "unknown",
            content: "checkout_v2: true\n",
            config_facts: &config_facts,
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "checkout_v2");
        assert_eq!(records[0].source_kind, "config_key");
        assert_eq!(records[0].edge_kind, "defines_config");
        assert_eq!(records[0].excerpt, "checkout_v2: true");
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
            config_facts: &[],
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
            config_facts: &[],
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
            config_facts: &[],
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
                config_facts: &[],
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
            config_facts: &[],
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
            config_facts: &[],
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
    fn extracts_sdk_feature_flag_keys() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.ts",
            language_id: "typescript",
            content: "const client = OpenFeature.getClient();\nif (client.getNumberValue(\"checkout_ratio\", 0) > 0) {}\nif (OpenFeature.getClient().getBooleanValue(\"factory_checkout\", false)) {}\nif (openFeature.getBooleanValue(\"checkout_v2\", false)) {}\nlet variant = ldClient.variation(\"payment_flow\", false);\nif (unleash.isEnabled(\"orders_v3\")) {}\nconst bucket = unleash.getVariant(\"search.experiment\");\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        assert!(records.iter().any(|record| {
            record.source_kind == "sdk_flag_key"
                && record.source_key == "checkout_ratio"
                && record.edge_kind == "guards_code"
        }));
        assert!(records.iter().any(|record| {
            record.source_kind == "sdk_flag_key"
                && record.source_key == "checkout_v2"
                && record.edge_kind == "guards_code"
        }));
        assert!(
            records
                .iter()
                .any(|record| record.source_key == "factory_checkout")
        );
        assert!(records.iter().any(|record| {
            record.source_kind == "sdk_flag_key"
                && record.source_key == "payment_flow"
                && record.edge_kind == "reads_config"
        }));
        assert!(
            records
                .iter()
                .any(|record| record.source_key == "orders_v3")
        );
        assert!(
            records
                .iter()
                .any(|record| record.source_key == "search.experiment")
        );
    }

    #[test]
    fn extracts_pascal_case_launchdarkly_sdk_feature_flag_keys() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/flags.go",
            language_id: "go",
            content: "client := ldclient.NewClient(\"sdk-key\", 5*time.Second)\nif client.BoolVariationCtx(ctx, \"go_checkout\", user, false) {}\nname := ldClient.StringVariation(\"csharp_checkout\", user, \"off\")\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        assert!(records.iter().any(|record| {
            record.source_kind == "sdk_flag_key" && record.source_key == "go_checkout"
        }));
        assert!(
            records
                .iter()
                .any(|record| record.source_key == "csharp_checkout")
        );
    }

    #[test]
    fn ignores_sdk_default_literals_when_flag_key_is_dynamic() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.ts",
            language_id: "typescript",
            content: "const client = OpenFeature.getClient();\nconst value = client.getStringValue(flagName, \"off\");\nconst name = ldClient.StringVariation(flagKey, user, \"fallback\");\nconst go = ldClient.BoolVariationCtx(ctx, \"go_checkout\", user, false);\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "go_checkout");
    }

    #[test]
    fn removes_tracked_sdk_receiver_after_reassignment() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.ts",
            language_id: "typescript",
            content: "const client = OpenFeature.getClient();\nclient.getBooleanValue(\"checkout_v2\", false);\nclient = chart.create();\nclient.variation(\"daily\", false);\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "checkout_v2");
    }

    #[test]
    fn tracks_typed_optional_and_constructed_sdk_receivers() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.ts",
            language_id: "typescript",
            content: "const client: OpenFeatureClient = OpenFeature.getClient();\nclient?.getBooleanValue(\"typed_checkout\", false);\noptions.client = chart.create();\nclient.getStringValue(\"still_tracked\", \"off\");\nvar ld = new LDClient(sdkKey);\nld.BoolVariation(\"constructed_checkout\", user, false);\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        for source_key in ["typed_checkout", "still_tracked", "constructed_checkout"] {
            assert!(
                records.iter().any(|record| record.source_key == source_key),
                "{source_key} should be extracted"
            );
        }
        assert!(
            !records.iter().any(|record| record.source_key == "chart"),
            "property assignments must not clear or emit local SDK receiver facts"
        );
    }

    #[test]
    fn tracks_property_multibinding_multiline_and_non_js_sdk_receivers() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/flags.go",
            language_id: "go",
            content: "this.client = OpenFeature.getClient();\nthis.client.getBooleanValue(\"property_checkout\", false);\nchart.client.variation(\"daily\", false);\nclient, err := ldclient.NewClient(\"sdk-key\", 5*time.Second)\nclient.BoolVariationCtx(ctx, \"multi_binding_checkout\", user, false)\nopenClient := openfeature.NewClient(\"svc\")\nopenClient.BooleanValue(ctx, \"go_openfeature_client_checkout\", false, opts)\nif (client.getBooleanValue(\n  \"multiline_checkout\",\n  false,\n)) {}\nclient.BooleanValue(\n  ctx,\n  \"multiline_second_arg\",\n  false,\n)\nunleash.IsEnabled(\"pascal_unleash\", opts)\nunleash.GetVariant(\"pascal_variant\", opts)\nopenFeature.GetBooleanValueAsync(\"dotnet_checkout\", false);\nopenFeature.BooleanValue(ctx, \"go_openfeature_checkout\", false, opts);\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        for source_key in [
            "property_checkout",
            "multi_binding_checkout",
            "go_openfeature_client_checkout",
            "multiline_checkout",
            "multiline_second_arg",
            "pascal_unleash",
            "pascal_variant",
            "dotnet_checkout",
            "go_openfeature_checkout",
        ] {
            assert!(
                records.iter().any(|record| record.source_key == source_key),
                "{source_key} should be extracted"
            );
        }
        assert!(
            !records.iter().any(|record| record.source_key == "sdk-key"),
            "SDK constructor credentials must not be extracted as feature flags"
        );
        assert!(
            !records.iter().any(|record| record.source_key == "daily"),
            "tracked property receivers must not enable unrelated properties with the same leaf"
        );
        assert!(records.iter().any(|record| {
            record.source_key == "multiline_checkout" && record.edge_kind == "guards_code"
        }));
    }

    #[test]
    fn ignores_sdk_shapes_inside_template_strings_and_preserves_statement_order() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.ts",
            language_id: "typescript",
            content: "const primary = OpenFeature.getClient();\nconsole.log(`primary.getBooleanValue(\"docs_flag\", false)`);\nconst textClient = \"OpenFeature.getClient()\";\ntextClient.variation(\"text_daily\", false);\nconst statusClient = openFeatureInitialized ? chart.create() : fallback;\nstatusClient.variation(\"status_daily\", false);\nconst Client = OpenFeature.getClient();\nclient.variation(\"case_daily\", false);\nconsole.log(\"other = OpenFeature.getClient()\"); other.variation(\"daily\", false);\nchart.variation(\"chart_daily\", false); const chart = OpenFeature.getClient();\nchart.getBooleanValue(\"real_chart_flag\", false);\nprimary.getStringValue(\n  flagName,\n  \"off\",\n);\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        assert!(records.iter().any(|record| {
            record.source_kind == "sdk_flag_key" && record.source_key == "real_chart_flag"
        }));
        for source_key in [
            "docs_flag",
            "text_daily",
            "status_daily",
            "case_daily",
            "daily",
            "chart_daily",
            "off",
        ] {
            assert!(
                !records.iter().any(|record| record.source_key == source_key),
                "{source_key} should not be extracted"
            );
        }
    }

    #[test]
    fn respects_sdk_receiver_order_multiline_openers_and_scope_shadowing() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.ts",
            language_id: "typescript",
            content: "const client = OpenFeature.getClient(), enabled = client.getBooleanValue(\"comma_checkout\", false);\nclient.BooleanValue(ctx,\n  \"opener_arg_checkout\",\n  false,\n)\nOpenFeature.getClient().getBooleanValue(\"direct_factory_checkout\", false);\nwrap(OpenFeature.getClient()).variation(\"wrapped_daily\", false);\nfunction setup(){ const local = OpenFeature.getClient(); local.getBooleanValue(\"local_checkout\", false); }\nlocal.getBooleanValue(\"leaked_local_checkout\", false);\nfunction render(client){ client.variation(\"shadowed_daily\", false); }\nclient.getBooleanValue(\"outer_checkout\", false);\nconst templateFlag = `${client.getBooleanValue(\"template_checkout\", false)}`;\nconst commentTemplate = `${/* client.variation(\"comment_template\", false) */ client.getBooleanValue(\"real_template\", false)}`;\nconst nestedTemplate = `${`raw client.variation(\"nested_template_raw\", false) ${client.getBooleanValue(\"nested_template_real\", false)}`}`;\nconst docs = `\nclient.getBooleanValue(\"multiline_template_raw\", false)\n${client.getBooleanValue(\"multiline_template_real\", false)}\n`;\nfunction renderAgain(client) {\n  client.variation(\"multiline_shadowed\", false);\n}\nclient.getBooleanValue(\"after_multiline_shadow\", false);\nfunction renderLater(client)\n{\n  client.variation(\"next_line_shadowed\", false);\n}\nclient.getBooleanValue(\"after_next_line_shadow\", false);\nconst cb = (client) => client.variation(\"arrow_daily\", false);\nconst cb2 = (client) => {\n  client.variation(\"arrow_body_daily\", false);\n};\nclient.getBooleanValue(\"after_arrow\", false);\nif (ready) {\nfunction nested(client) {\n  client.variation(\"nested_shadow\", false);\n}\nclient.getBooleanValue(\"after_nested_shadow\", false);\n}\nchart.getBooleanValue(\"daily\"); client.getBooleanValue(\n  \"later_pending_checkout\",\n  false,\n)\nclient.getBooleanValue(\n// rollout key\n  \"comment_gap_checkout\",\n  false,\n)\nthis.client = OpenFeature.getClient(); this.client = chart; this.client.variation(\"property_daily\", false);\nconst services = { client: OpenFeature.getClient() }; services.variation(\"services_daily\", false);\nconst bogus = buildClient.newClient(); bogus.variation(\"builder_daily\", false);\nconst apiClient = OpenFeatureAPI.getInstance().getClient(); apiClient.getBooleanValue(\"java_api_checkout\", false);\nconst dotnet = OpenFeature.Api.Instance.GetClient(); dotnet.GetBooleanValueAsync(\"dotnet_api_checkout\", false);\nopenFeature.get_boolean_value(\"python_openfeature\", False);\nunleash.is_enabled(\"python_unleash\");\nunleash.get_variant(\"python_variant\");\nconst ldpy = ldclient.get(); ldpy.variation(\"python_ld\", user, false);\nldclient.get().variation(\"python_direct\", user, false);\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        for source_key in [
            "comma_checkout",
            "opener_arg_checkout",
            "direct_factory_checkout",
            "local_checkout",
            "outer_checkout",
            "template_checkout",
            "real_template",
            "nested_template_real",
            "multiline_template_real",
            "after_multiline_shadow",
            "after_next_line_shadow",
            "after_arrow",
            "after_nested_shadow",
            "later_pending_checkout",
            "comment_gap_checkout",
            "java_api_checkout",
            "dotnet_api_checkout",
            "python_openfeature",
            "python_unleash",
            "python_variant",
            "python_ld",
            "python_direct",
        ] {
            assert!(
                records.iter().any(|record| record.source_key == source_key),
                "{source_key} should be extracted"
            );
        }
        for source_key in [
            "wrapped_daily",
            "leaked_local_checkout",
            "comment_template",
            "nested_template_raw",
            "multiline_template_raw",
            "shadowed_daily",
            "multiline_shadowed",
            "next_line_shadowed",
            "arrow_daily",
            "arrow_body_daily",
            "nested_shadow",
            "daily",
            "property_daily",
            "services_daily",
            "builder_daily",
        ] {
            assert!(
                !records.iter().any(|record| record.source_key == source_key),
                "{source_key} should not be extracted"
            );
        }
    }

    #[test]
    fn ignores_sdk_feature_flag_keys_inside_comments_and_strings() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/app.ts",
            language_id: "typescript",
            content: "const client = service.create();\nconsole.log(\"client.variation('string_only', false)\");\n// openFeature.getBooleanValue(\"commented\", false)\nchart.variation(\"daily\");\npermission.isEnabled(\"camera\");\nclient.variation(\"ordinary_client\", false);\nif (featureFlags.boolVariation(\"live_flag\", false)) {}\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_key, "live_flag");
        assert_eq!(records[0].source_kind, "sdk_flag_key");
    }

    #[test]
    fn non_javascript_function_parameters_shadow_tracked_sdk_receivers() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "service/main.go",
            language_id: "go",
            content: "client := ldclient.NewClient()\nfunc render(client *http.Client) {\n  client.BoolVariation(\"go_shadowed\", user, false)\n}\nclient.BoolVariation(\"go_outer\", user, false)\nclient.BoolVariation(\"go_same_line\", user, false); audit(client);\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        for source_key in ["go_outer", "go_same_line"] {
            assert!(
                records.iter().any(|record| record.source_key == source_key),
                "{source_key} should be extracted"
            );
        }
        assert!(
            !records
                .iter()
                .any(|record| record.source_key == "go_shadowed")
        );
    }

    #[test]
    fn extracts_additional_environment_source_key_shapes() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "src/env.ts",
            language_id: "typescript",
            content: "const a = Deno.env.get(\"DENO_FLAG\");\nconst b = Bun.env.BUN_FLAG;\nconst c = import.meta.env.VITE_FLAG;\nconst d = `${enabled ?\n  process.env.CHECKOUT_V2\n  : \"\"}`;\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        for source_key in ["DENO_FLAG", "BUN_FLAG", "VITE_FLAG", "CHECKOUT_V2"] {
            assert!(
                records.iter().any(|record| record.source_key == source_key),
                "{source_key} should be extracted"
            );
        }
    }

    #[test]
    fn extracts_boolean_flags_from_inline_config_objects() {
        let records = extract_feature_flags(FeatureFlagFileInput {
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "config/flags.yaml",
            language_id: "unknown",
            content: "flags: { checkout_v2: true, payments_v2: false }\n{\"docs_url\":\"https://example.test/true\",\"search_v2\":true,\"feed_v2\":false}\nfeatures: [true, false]\n{\"permissions\":[\"read\",true]}\n",
            config_facts: &[],
        })
        .expect("feature flag records should extract");

        assert!(records.iter().any(|record| {
            record.source_key == "checkout_v2" && record.edge_kind == "defines_config"
        }));
        assert!(records.iter().any(|record| {
            record.source_key == "payments_v2" && record.edge_kind == "defines_config"
        }));
        assert!(records.iter().any(|record| {
            record.source_key == "search_v2" && record.edge_kind == "defines_config"
        }));
        assert!(records.iter().any(|record| {
            record.source_key == "feed_v2" && record.edge_kind == "defines_config"
        }));
        assert!(
            !records.iter().any(|record| record.source_key == "https"),
            "boolean words inside quoted strings must not emit config keys"
        );
        assert!(
            !records.iter().any(|record| record.source_key == "features"
                || record.source_key == "permissions"),
            "boolean array elements must not emit parent config keys"
        );
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
            config_facts: &[],
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
        assert_eq!(
            env_keys("if os.environ['PY_FLAG'] and ENV['RUBY_FLAG'] {"),
            vec!["PY_FLAG".to_owned(), "RUBY_FLAG".to_owned()]
        );
        assert_eq!(
            env_keys("DEFAULT_ENV['theme']; TEST_ENV['fixture']; ENV['REAL_FLAG'];"),
            vec!["REAL_FLAG".to_owned()]
        );
        assert_eq!(
            env_keys("const key = `${process.env.CHECKOUT_V2}`;"),
            vec!["CHECKOUT_V2".to_owned()]
        );
        assert_eq!(
            env_keys("const url = `https://host/${process.env.CHECKOUT_V2}`;"),
            vec!["CHECKOUT_V2".to_owned()]
        );
        assert_eq!(
            env_keys("const key = `${{enabled: true}.enabled && process.env.CHECKOUT_V2}`;"),
            vec!["CHECKOUT_V2".to_owned()]
        );
        assert_eq!(
            env_keys("const key = `${/* process.env.FAKE */ process.env.CHECKOUT_V2}`;"),
            vec!["CHECKOUT_V2".to_owned()]
        );
        assert_eq!(
            env_keys("const key = `${`raw process.env.FAKE ${process.env.CHECKOUT_V2}`}`;"),
            vec!["CHECKOUT_V2".to_owned()]
        );
        assert_eq!(
            env_keys("const key = `set process.env.CHECKOUT_V2`;"),
            Vec::<String>::new()
        );
    }
}
