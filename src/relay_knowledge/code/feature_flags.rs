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
#[path = "feature_flags/tests.rs"]
mod tests;
