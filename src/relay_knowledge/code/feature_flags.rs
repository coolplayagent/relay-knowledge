use std::collections::BTreeMap;

use crate::domain::{CodeFeatureFlagRecord, DomainError, RepositoryCodeRange};

use super::stable_id;

const CONFIG_RECEIVERS: &[&str] = &[
    "config",
    "settings",
    "feature_flags",
    "flags",
    "toggles",
    "options",
];
const CONFIG_METHODS: &[&str] = &[
    ".get(",
    ".get_bool(",
    ".getBoolean(",
    ".get_boolean(",
    ".enabled(",
    ".is_enabled(",
];
const CONFIG_EXTENSIONS: &[&str] = &[
    ".toml",
    ".yaml",
    ".yml",
    ".json",
    ".env",
    ".ini",
    ".properties",
];

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
        collect_line_records(
            &mut records,
            LineContext {
                input: &input,
                line,
                line_number: line_index.saturating_add(1),
                byte_start,
                config_file,
                skip_for_comment: comment_state.skip_line(line, &input),
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
    line_number: usize,
    byte_start: usize,
    config_file: bool,
    skip_for_comment: bool,
}

fn collect_line_records(
    records: &mut Vec<CodeFeatureFlagRecord>,
    context: LineContext<'_, '_>,
) -> Result<(), DomainError> {
    if context.skip_for_comment {
        return Ok(());
    }

    let mut line_records = Vec::new();
    for key in preprocessor_flag_keys(context.line, context.input.language_id) {
        line_records.push(("preprocessor_symbol", key, "guards_code"));
    }
    for key in env_keys(context.line) {
        line_records.push(("env_var", key, usage_edge_kind(context.line)));
    }
    for key in config_read_keys(context.line) {
        line_records.push(("config_key", key, usage_edge_kind(context.line)));
    }
    if context.config_file
        && let Some(key) = boolean_config_key(context.line)
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

#[derive(Default)]
struct CommentState {
    in_block_comment: bool,
}

impl CommentState {
    fn skip_line(&mut self, line: &str, input: &FeatureFlagFileInput<'_>) -> bool {
        let trimmed = line.trim_start();
        if self.in_block_comment {
            if trimmed.contains("*/") {
                self.in_block_comment = false;
            }
            return true;
        }
        if trimmed.starts_with("/*") {
            self.in_block_comment = !trimmed.contains("*/");
            return true;
        }

        trimmed.starts_with("//")
            || trimmed.starts_with('*')
            || (trimmed.starts_with('#') && hash_starts_comment(input))
    }
}

fn hash_starts_comment(input: &FeatureFlagFileInput<'_>) -> bool {
    if looks_like_config_file(input.path) {
        return true;
    }

    matches!(
        input.language_id,
        "python" | "ruby" | "bash" | "php" | "unknown"
    )
}

fn env_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for pattern in [
        "std::env::var(",
        "std::env::var_os(",
        "env::var(",
        "os.getenv(",
        "System.getenv(",
    ] {
        collect_quoted_arguments(line, pattern, &mut keys);
    }
    collect_dotted_members(line, "process.env.", &mut keys);
    collect_bracket_keys(line, "process.env[", &mut keys);

    keys
}

fn config_read_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for receiver in CONFIG_RECEIVERS {
        for method in CONFIG_METHODS {
            collect_quoted_arguments(line, &format!("{receiver}{method}"), &mut keys);
        }
    }

    keys
}

fn preprocessor_flag_keys(line: &str, language_id: &str) -> Vec<String> {
    if !matches!(language_id, "c" | "cpp" | "csharp") {
        return Vec::new();
    }
    let trimmed = line.trim_start();
    let remainder = if let Some(remainder) = trimmed.strip_prefix("#ifdef") {
        remainder
    } else if let Some(remainder) = trimmed.strip_prefix("#ifndef") {
        remainder
    } else if let Some(remainder) = trimmed.strip_prefix("#if") {
        remainder
    } else {
        return Vec::new();
    };

    let mut keys = Vec::new();
    collect_defined_preprocessor_keys(remainder, &mut keys);
    if let Some(key) = first_preprocessor_identifier(remainder) {
        push_unique(&mut keys, key);
    }

    keys
}

fn collect_defined_preprocessor_keys(value: &str, keys: &mut Vec<String>) {
    let mut start = 0usize;
    while let Some(relative_index) = value[start..].find("defined") {
        let defined_start = start + relative_index + "defined".len();
        let after_defined = value[defined_start..].trim_start();
        let candidate = if let Some(remainder) = after_defined.strip_prefix('(') {
            remainder
                .split_once(')')
                .map(|(identifier, _)| identifier.trim())
        } else {
            Some(
                after_defined
                    .split(|character: char| {
                        !(character.is_ascii_alphanumeric() || character == '_')
                    })
                    .next()
                    .unwrap_or_default(),
            )
        };
        if let Some(identifier) = candidate
            && valid_preprocessor_key(identifier)
        {
            push_unique(keys, identifier.to_owned());
        }
        start = defined_start.saturating_add(1);
    }
}

fn first_preprocessor_identifier(value: &str) -> Option<String> {
    let identifier = value
        .trim_start()
        .trim_start_matches('(')
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())?;
    valid_preprocessor_key(identifier).then(|| identifier.to_owned())
}

fn valid_preprocessor_key(key: &str) -> bool {
    valid_source_key(key)
        && key
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && !matches!(key, "defined" | "if" | "ifdef" | "ifndef")
}

fn collect_quoted_arguments(line: &str, pattern: &str, keys: &mut Vec<String>) {
    let mut start = 0usize;
    while let Some(relative_index) = line[start..].find(pattern) {
        let value_start = start + relative_index + pattern.len();
        if let Some(key) = quoted_prefix(&line[value_start..]) {
            push_unique(keys, key);
        }
        start = value_start.saturating_add(1);
    }
}

fn collect_bracket_keys(line: &str, pattern: &str, keys: &mut Vec<String>) {
    collect_quoted_arguments(line, pattern, keys);
}

fn collect_dotted_members(line: &str, pattern: &str, keys: &mut Vec<String>) {
    let mut start = 0usize;
    while let Some(relative_index) = line[start..].find(pattern) {
        let member_start = start + relative_index + pattern.len();
        let member = line[member_start..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if valid_source_key(&member) {
            push_unique(keys, member.clone());
        }
        start = member_start.saturating_add(member.len().max(1));
    }
}

fn boolean_config_key(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let separator = trimmed
        .find('=')
        .or_else(|| trimmed.find(':'))
        .filter(|index| *index > 0)?;
    let (key, value) = trimmed.split_at(separator);
    let key = key.trim().trim_matches('"').trim_matches('\'');
    let value = value[1..]
        .trim()
        .trim_end_matches(',')
        .trim_matches('"')
        .trim_matches('\'');
    if !matches!(value, "true" | "false" | "enabled" | "disabled") || key.is_empty() {
        return None;
    }
    if key
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        Some(key.to_owned())
    } else {
        None
    }
}

fn quoted_prefix(value: &str) -> Option<String> {
    let value = value.trim_start();
    let mut chars = value.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = value[1..].find(quote)?;
    let key = &value[1..1 + end];
    valid_source_key(key).then(|| key.to_owned())
}

fn valid_source_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 160
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
        })
}

fn usage_edge_kind(line: &str) -> &'static str {
    if line_looks_conditional(line) {
        "guards_code"
    } else {
        "reads_config"
    }
}

fn line_looks_conditional(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("if ")
        || trimmed.starts_with("if(")
        || trimmed.starts_with("elif ")
        || trimmed.starts_with("else if")
        || trimmed.starts_with("while ")
        || trimmed.contains(" if ")
        || trimmed.contains(" ? ")
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

fn looks_like_config_file(path: &str) -> bool {
    CONFIG_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
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
