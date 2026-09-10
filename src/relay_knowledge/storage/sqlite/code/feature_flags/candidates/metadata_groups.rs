//! Metadata admission follows bounded alias groups before the ranked seed limit.
use crate::domain::CodeFeatureFlagRequest;
use rusqlite::types::Value;

pub(super) struct Plan {
    pub(super) prefix: String,
    pub(super) state: String,
    pub(super) values: Vec<Value>,
}

pub(super) fn plan(
    scope: &str,
    parameters: &[Value],
    request: &CodeFeatureFlagRequest,
    rounds: usize,
    identities: usize,
) -> Option<Plan> {
    if request.query.is_none()
        || (request.domain.is_none() && request.source.is_none() && request.hot_reload.is_none())
    {
        return None;
    }
    let kinds_scope = scope.replace("flag.", "kind.");
    let links_scope = scope.replace("flag.", "link.");
    let identity_scope = scope.replace("flag.", "identity.");
    let metadata_scope = scope.replace("flag.", "metadata.");
    let prefix = format!("kinds AS MATERIALIZED (SELECT DISTINCT kind.source_kind FROM code_repository_feature_flags kind WHERE {kinds_scope}),
        links AS NOT MATERIALIZED (SELECT link.source_key, binding.value AS symbol
        FROM code_repository_feature_flags link CROSS JOIN json_each(
            CASE WHEN link.source_kind IN ('config_symbol', 'config_getter')
            THEN CASE WHEN coalesce(json_extract(link.metadata_json, '$.referenced_symbol'), link.source_key) = link.source_key
                THEN json_insert(CASE WHEN json_type(link.metadata_json, '$.bindings') = 'array' THEN link.metadata_json ELSE json_set(link.metadata_json, '$.bindings', json('[]')) END, '$.bindings[#]', link.source_key)
                ELSE json_insert(CASE WHEN json_type(link.metadata_json, '$.bindings') = 'array' THEN link.metadata_json ELSE json_set(link.metadata_json, '$.bindings', json('[]')) END, '$.bindings[#]', link.source_key, '$.bindings[#]', json_extract(link.metadata_json, '$.referenced_symbol')) END
            ELSE link.metadata_json END, '$.bindings') binding
        WHERE {links_scope} AND link.source_kind IN (SELECT source_kind FROM kinds))");
    let representative = |key: &str| {
        format!("(SELECT MIN(identity.rowid) FROM code_repository_feature_flags identity
        WHERE {identity_scope} AND identity.source_kind IN (SELECT source_kind FROM kinds) AND identity.source_key = {key})")
    };
    let initial = representative("flag.source_key");
    let next = representative("rhs.source_key");
    let mut values = Vec::new();
    // Prefix scopes, three representative lookups, then metadata scope occur in
    // this order in SQL. Every relation repeats the full authorization predicate.
    for _ in 0..6 {
        values.extend_from_slice(parameters);
    }
    let mut metadata = vec![metadata_scope];
    for (field, value) in [
        ("domain", &request.domain),
        ("source_format", &request.source),
    ] {
        if let Some(value) = value {
            metadata.push(format!(
                "json_extract(metadata.metadata_json, '$.{field}') = ?"
            ));
            values.push(Value::Text(value.clone()));
        }
    }
    if let Some(value) = request.hot_reload {
        metadata.push("json_extract(metadata.metadata_json, '$.hot_reload') = ?".to_owned());
        values.push(Value::Integer(i64::from(value)));
    }
    // Rowids identify authorized source-key groups. The recursive work table
    // stores integers, not copies of arbitrarily long source keys or metadata.
    // At most rounds+1 states represent one identity. The extra row therefore
    // proves identity overflow, never an apparently complete truncated closure.
    // One aggregate materialization prevents SQLite from replaying the correlated
    // closure separately for count, frontier and metadata checks. Both JSON arrays
    // contain at most `states` integer rowids; no source text is materialized.
    let states = identities * (rounds + 1) + 1;
    let state = format!(
        "(WITH RECURSIVE reach(node_id, depth) AS (
        SELECT {initial}, 0 UNION
        SELECT {next}, reach.depth + 1 FROM reach
        JOIN code_repository_feature_flags current ON current.rowid = reach.node_id
        CROSS JOIN links lhs ON lhs.source_key = current.source_key
        CROSS JOIN links rhs ON rhs.symbol = lhs.symbol
        WHERE reach.depth < {rounds} AND rhs.source_key <> current.source_key LIMIT {states}),
        summary AS MATERIALIZED (SELECT COUNT(DISTINCT node_id) AS total,
            json_group_array(DISTINCT node_id) AS seen,
            json_group_array(DISTINCT CASE WHEN depth = {rounds} THEN node_id END) AS frontier
            FROM reach)
        SELECT CASE WHEN summary.total > {identities} THEN 2
        WHEN EXISTS (SELECT 1 FROM json_each(summary.frontier) frontier
            JOIN code_repository_feature_flags current ON current.rowid = frontier.value
            CROSS JOIN links lhs ON lhs.source_key = current.source_key
            CROSS JOIN links rhs ON rhs.symbol = lhs.symbol
            WHERE {next} NOT IN (SELECT value FROM json_each(summary.seen))) THEN 2
        WHEN EXISTS (SELECT 1 FROM json_each(summary.seen) visited
            JOIN code_repository_feature_flags current ON current.rowid = visited.value
            JOIN code_repository_feature_flags metadata ON metadata.source_key = current.source_key
            AND metadata.source_kind IN (SELECT source_kind FROM kinds)
            WHERE {}) THEN 1 ELSE 0 END FROM summary)",
        metadata.join(" AND ")
    );
    Some(Plan {
        prefix,
        state,
        values,
    })
}
