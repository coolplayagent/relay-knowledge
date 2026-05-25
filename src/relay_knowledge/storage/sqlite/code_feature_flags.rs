use std::collections::BTreeMap;

use rusqlite::{Connection, Transaction, params};

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRecord, CodeFeatureFlagRequest, CodeFeatureFlagUsage,
        CodeRepositoryStatus, RepositoryCodeChunkRecord, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    SearchDocumentInserter,
    code_query_hits::{required_repository, selected_row},
};

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

pub(super) fn insert_from_chunks(
    transaction: &Transaction<'_>,
    chunks: &[RepositoryCodeChunkRecord],
) -> Result<(), StorageError> {
    let records = records_from_chunks(chunks)?;
    insert_records(transaction, &records)
}

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    records: &[CodeFeatureFlagRecord],
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_feature_flags (
            repository_id, source_scope, feature_flag_id, usage_id, file_id, path, language_id,
            name, source_kind, source_key, edge_kind, confidence_basis_points, confidence_tier,
            byte_start, byte_end, line_start, line_end, excerpt
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
        ",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for record in records {
        statement.execute(params![
            record.repository_id,
            record.source_scope,
            record.feature_flag_id,
            record.usage_id,
            record.file_id,
            record.path,
            record.language_id,
            record.name,
            record.source_kind,
            record.source_key,
            record.edge_kind,
            record.confidence_basis_points,
            record.confidence_tier,
            record.byte_range.start,
            record.byte_range.end,
            record.line_range.start,
            record.line_range.end,
            record.excerpt,
        ])?;
        search_documents.insert(
            &record.source_scope,
            "feature_flag",
            &record.usage_id,
            &record.path,
            &record.language_id,
            [
                record.name.as_str(),
                record.source_kind.as_str(),
                record.source_key.as_str(),
                record.edge_kind.as_str(),
                record.excerpt.as_str(),
                record.path.as_str(),
            ],
        )?;
    }

    Ok(())
}

fn records_from_chunks(
    chunks: &[RepositoryCodeChunkRecord],
) -> Result<Vec<CodeFeatureFlagRecord>, StorageError> {
    let mut records = Vec::new();
    for chunk in chunks {
        let mut byte_start = chunk.byte_range.start as usize;
        let config_file = looks_like_config_file(&chunk.path);
        for (line_index, line) in chunk.content.lines().enumerate() {
            let line_number = (chunk.line_range.start as usize).saturating_add(line_index);
            collect_line_records(
                &mut records,
                LineContext {
                    chunk,
                    line,
                    line_number,
                    byte_start,
                    config_file,
                },
            )?;
            byte_start = byte_start.saturating_add(line.len()).saturating_add(1);
        }
    }

    let mut deduped = BTreeMap::new();
    for record in records {
        deduped.insert(record.usage_id.clone(), record);
    }

    Ok(deduped.into_values().collect())
}

struct LineContext<'a> {
    chunk: &'a RepositoryCodeChunkRecord,
    line: &'a str,
    line_number: usize,
    byte_start: usize,
    config_file: bool,
}

fn collect_line_records(
    records: &mut Vec<CodeFeatureFlagRecord>,
    context: LineContext<'_>,
) -> Result<(), StorageError> {
    let mut line_records = Vec::new();
    if let Some(key) = env_key(context.line) {
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
    context: &LineContext<'_>,
    source_kind: &str,
    source_key: &str,
    edge_kind: &str,
) -> Result<CodeFeatureFlagRecord, StorageError> {
    let chunk = context.chunk;
    let byte_end = context.byte_start.saturating_add(context.line.len());
    let byte_range =
        RepositoryCodeRange::new("feature_flag_byte_range", context.byte_start, byte_end)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let line_range = RepositoryCodeRange::new(
        "feature_flag_line_range",
        context.line_number,
        context.line_number,
    )
    .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let feature_flag_id = stable_id(
        "feature_flag",
        [
            chunk.repository_id.as_str(),
            chunk.source_scope.as_str(),
            source_kind,
            source_key,
        ],
    );
    let usage_id = stable_id(
        "feature_flag_usage",
        [
            chunk.repository_id.as_str(),
            chunk.source_scope.as_str(),
            chunk.path.as_str(),
            source_kind,
            source_key,
            edge_kind,
            &context.line_number.to_string(),
        ],
    );

    Ok(CodeFeatureFlagRecord {
        repository_id: chunk.repository_id.clone(),
        source_scope: chunk.source_scope.clone(),
        feature_flag_id,
        usage_id,
        file_id: chunk.file_id.clone(),
        path: chunk.path.clone(),
        language_id: chunk.language_id.clone(),
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

pub(super) fn search(
    connection: &mut Connection,
    request: CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    super::super::retry::retry_sqlite_transient(|| {
        search_with_status(connection, &status, &request)
    })
}

fn search_with_status(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    let source_scope = status.last_indexed_scope_id.as_deref().ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' does not have an indexed source scope",
            status.alias
        ))
    })?;
    let terms = request
        .query
        .as_deref()
        .map(query_terms)
        .unwrap_or_default();
    let retrieval_request = retrieval_like_request(request)?;
    let mut statement = connection.prepare(
        "
        SELECT flag.feature_flag_id, flag.usage_id, flag.file_id, flag.path, flag.language_id,
               flag.name, flag.source_kind, flag.source_key, flag.edge_kind,
               flag.confidence_basis_points, flag.confidence_tier,
               flag.byte_start, flag.byte_end, flag.line_start, flag.line_end, flag.excerpt,
               (
                   SELECT symbol_snapshot_id
                   FROM code_repository_symbols symbol
                   WHERE symbol.source_scope = flag.source_scope
                     AND symbol.path = flag.path
                     AND symbol.line_start <= flag.line_start
                     AND symbol.line_end >= flag.line_start
                   ORDER BY symbol.line_start DESC, symbol.line_end ASC
                   LIMIT 1
               ) AS related_symbol_snapshot_id,
               (
                   SELECT name
                   FROM code_repository_symbols symbol
                   WHERE symbol.source_scope = flag.source_scope
                     AND symbol.path = flag.path
                     AND symbol.line_start <= flag.line_start
                     AND symbol.line_end >= flag.line_start
                   ORDER BY symbol.line_start DESC, symbol.line_end ASC
                   LIMIT 1
               ) AS related_symbol_name
        FROM code_repository_feature_flags flag
        WHERE flag.source_scope = ?1
        ORDER BY flag.name ASC,
                 CASE flag.edge_kind
                   WHEN 'guards_code' THEN 0
                   WHEN 'defines_config' THEN 1
                   ELSE 2
                 END,
                 flag.path ASC,
                 flag.line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(FeatureFlagRow {
            feature_flag_id: row.get(0)?,
            usage_id: row.get(1)?,
            file_id: row.get(2)?,
            path: row.get(3)?,
            language_id: row.get(4)?,
            name: row.get(5)?,
            source_kind: row.get(6)?,
            source_key: row.get(7)?,
            edge_kind: row.get(8)?,
            confidence_basis_points: row.get(9)?,
            confidence_tier: row.get(10)?,
            byte_range: RepositoryCodeRange {
                start: row.get(11)?,
                end: row.get(12)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(13)?,
                end: row.get(14)?,
            },
            excerpt: row.get(15)?,
            related_symbol_snapshot_id: row.get(16)?,
            related_symbol_name: row.get(17)?,
        })
    })?;
    let mut groups = BTreeMap::<String, CodeFeatureFlagGraph>::new();
    for row in rows {
        let row = row?;
        if !selected_row(&row.path, &row.language_id, status, &retrieval_request) {
            continue;
        }
        if !terms.is_empty() && !row_matches_terms(&row, &terms) {
            continue;
        }
        let score = score_row(&row, &terms);
        let group = groups
            .entry(row.feature_flag_id.clone())
            .or_insert_with(|| CodeFeatureFlagGraph {
                feature_flag_id: row.feature_flag_id.clone(),
                name: row.name.clone(),
                source_kind: row.source_kind.clone(),
                source_key: row.source_key.clone(),
                score,
                usages: Vec::new(),
            });
        group.score = group.score.max(score);
        group.usages.push(CodeFeatureFlagUsage {
            usage_id: row.usage_id,
            path: row.path,
            language_id: row.language_id,
            file_id: row.file_id,
            byte_range: row.byte_range,
            line_range: row.line_range,
            edge_kind: row.edge_kind,
            related_symbol_snapshot_id: row.related_symbol_snapshot_id,
            related_symbol_name: row.related_symbol_name,
            confidence_basis_points: row.confidence_basis_points,
            confidence_tier: row.confidence_tier,
            excerpt: row.excerpt,
        });
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    for group in &mut groups {
        group.usages.sort_by(|left, right| {
            edge_priority(&left.edge_kind)
                .cmp(&edge_priority(&right.edge_kind))
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.line_range.start.cmp(&right.line_range.start))
        });
    }
    groups.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.source_key.cmp(&right.source_key))
    });
    groups.truncate(request.limit);

    Ok(groups)
}

#[derive(Debug)]
struct FeatureFlagRow {
    feature_flag_id: String,
    usage_id: String,
    file_id: String,
    path: String,
    language_id: String,
    name: String,
    source_kind: String,
    source_key: String,
    edge_kind: String,
    confidence_basis_points: u16,
    confidence_tier: String,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
    excerpt: String,
    related_symbol_snapshot_id: Option<String>,
    related_symbol_name: Option<String>,
}

fn retrieval_like_request(
    request: &CodeFeatureFlagRequest,
) -> Result<crate::domain::CodeRetrievalRequest, StorageError> {
    crate::domain::CodeRetrievalRequest::new(
        request.query.clone().unwrap_or_else(|| "*".to_owned()),
        request.repository.clone(),
        crate::domain::CodeQueryKind::Hybrid,
        request.limit.clamp(1, 50),
        request.freshness_policy,
    )
    .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn row_matches_terms(row: &FeatureFlagRow, terms: &[String]) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {}",
        row.name, row.source_kind, row.source_key, row.edge_kind, row.path, row.excerpt
    )
    .to_ascii_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn score_row(row: &FeatureFlagRow, terms: &[String]) -> f64 {
    let edge_score = match row.edge_kind.as_str() {
        "guards_code" => 20.0,
        "defines_config" => 16.0,
        _ => 12.0,
    };
    let confidence = f64::from(row.confidence_basis_points) / 1000.0;
    let query_bonus = if terms.is_empty() {
        0.0
    } else if row_matches_terms(row, terms) {
        8.0
    } else {
        0.0
    };

    edge_score + confidence + query_bonus
}

fn edge_priority(edge_kind: &str) -> usize {
    match edge_kind {
        "guards_code" => 0,
        "defines_config" => 1,
        _ => 2,
    }
}

fn env_key(line: &str) -> Option<String> {
    for pattern in [
        "std::env::var(",
        "std::env::var_os(",
        "env::var(",
        "os.getenv(",
        "System.getenv(",
    ] {
        if let Some(key) = quoted_argument_after(line, pattern) {
            return Some(key);
        }
    }
    if let Some(key) = dotted_member_after(line, "process.env.") {
        return Some(key);
    }
    bracket_key_after(line, "process.env[")
}

fn config_read_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for receiver in CONFIG_RECEIVERS {
        for method in CONFIG_METHODS {
            let pattern = format!("{receiver}{method}");
            if let Some(key) = quoted_argument_after(line, &pattern) {
                push_unique(&mut keys, key);
            }
        }
    }

    keys
}

fn boolean_config_key(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with("/*") {
        return None;
    }
    let separator = trimmed
        .find('=')
        .or_else(|| trimmed.find(':'))
        .filter(|index| *index > 0)?;
    let (key, value) = trimmed.split_at(separator);
    let key = key.trim().trim_matches('"').trim_matches('\'');
    let value = value[1..].trim().trim_end_matches(',').trim_matches('"');
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

fn quoted_argument_after(line: &str, pattern: &str) -> Option<String> {
    let index = line.find(pattern)?;
    let remainder = &line[index + pattern.len()..];
    quoted_prefix(remainder)
}

fn bracket_key_after(line: &str, pattern: &str) -> Option<String> {
    let index = line.find(pattern)?;
    quoted_prefix(&line[index + pattern.len()..])
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

fn dotted_member_after(line: &str, pattern: &str) -> Option<String> {
    let index = line.find(pattern)?;
    let member = line[index + pattern.len()..]
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .collect::<String>();
    valid_source_key(&member).then_some(member)
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

fn stable_id<'a>(prefix: &str, parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }

    format!("{prefix}:{:016x}", stable_hash64(&bytes))
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
mod tests {
    use crate::domain::{RepositoryCodeChunkRecord, RepositoryCodeRange};

    use super::{config_read_keys, env_key, records_from_chunks};

    #[test]
    fn extracts_env_config_and_guarded_feature_flags_from_chunks() {
        let chunk = RepositoryCodeChunkRecord {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            chunk_id: "chunk".to_owned(),
            file_id: "file".to_owned(),
            path: "src/lib.rs".to_owned(),
            language_id: "rust".to_owned(),
            content: "if std::env::var(\"CHECKOUT_V2\").is_ok() {\nconfig.get_bool(\"payments.enabled\");".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 80 },
            line_range: RepositoryCodeRange { start: 10, end: 11 },
            symbol_snapshot_id: None,
        };

        let records = records_from_chunks(&[chunk]).expect("feature flag records should extract");
        let checkout = records
            .iter()
            .find(|record| record.source_key == "CHECKOUT_V2")
            .expect("checkout env flag should exist");
        let payments = records
            .iter()
            .find(|record| record.source_key == "payments.enabled")
            .expect("payments config flag should exist");

        assert_eq!(records.len(), 2);
        assert_eq!(checkout.edge_kind, "guards_code");
        assert_eq!(checkout.line_range.start, 10);
        assert_eq!(payments.edge_kind, "reads_config");
    }

    #[test]
    fn detects_common_source_key_shapes() {
        assert_eq!(
            env_key("const enabled = process.env.CHECKOUT_V2 === '1';").as_deref(),
            Some("CHECKOUT_V2")
        );
        assert_eq!(
            config_read_keys("if config.get_bool(\"checkout.v2\") {"),
            vec!["checkout.v2".to_owned()]
        );
    }
}
