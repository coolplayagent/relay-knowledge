use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareFile, SoftwareFileInput, SoftwareGlobalRequest,
        SoftwareRelationship, SoftwareRelationshipInput, SoftwareTopic, SoftwareTopicInput,
    },
    project::KNOWLEDGE_MAP_RELATIVE_PATH,
    storage::StorageError,
};

const PROJECTION_PAGE_SIZE: usize = 512;

pub(super) fn materialize_files(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<usize, StorageError> {
    let mut offset = 0;
    let mut count = 0;
    loop {
        let files = software_file_page(
            connection,
            source_scope,
            graph_version,
            PROJECTION_PAGE_SIZE,
            offset,
        )?;
        if files.is_empty() {
            break;
        }
        for file in &files {
            insert_file(connection, file)?;
        }
        let page_len = files.len();
        count += page_len;
        offset += page_len;
    }

    Ok(count)
}

fn software_file_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    offset: usize,
) -> Result<Vec<SoftwareFile>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT repository_id, source_scope, path, language_id, parse_status
        FROM code_repository_files
        WHERE source_scope = ?1
        ORDER BY path ASC
        LIMIT ?2 OFFSET ?3
        ",
    )?;
    let rows = statement.query_map(params![source_scope, limit as i64, offset as i64], |row| {
        let path = row.get::<_, String>(2)?;
        let language_id = row.get::<_, String>(3)?;
        Ok(SoftwareFileInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            file_role: file_role(&path, &language_id).to_owned(),
            path,
            language_id,
            parse_status: row.get(4)?,
            created_graph_version: graph_version,
        })
    })?;

    rows.map(|row| {
        row.map_err(StorageError::from).and_then(|input| {
            SoftwareFile::new(input).map_err(|error| StorageError::InvalidInput(error.to_string()))
        })
    })
    .collect()
}

pub(super) fn materialize_topics(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<usize, StorageError> {
    let mut offset = 0;
    loop {
        let topics = markdown_heading_topic_page(
            connection,
            source_scope,
            graph_version,
            PROJECTION_PAGE_SIZE,
            offset,
        )?;
        if topics.is_empty() {
            break;
        }
        for topic in &topics {
            insert_topic(connection, topic)?;
        }
        offset += topics.len();
    }

    let mut offset = 0;
    loop {
        let topics = knowledge_map_topic_page(
            connection,
            source_scope,
            graph_version,
            PROJECTION_PAGE_SIZE,
            offset,
        )?;
        if topics.is_empty() {
            break;
        }
        for topic in &topics {
            insert_topic(connection, topic)?;
        }
        offset += topics.len();
    }

    count_topics(connection, source_scope)
}

pub(super) fn materialize_relationships(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<usize, StorageError> {
    let mut count = 0;
    count += insert_relationship_batches(connection, |limit, offset| {
        document_relationship_page(connection, source_scope, graph_version, limit, offset)
    })?;
    count += insert_relationship_batches(connection, |limit, offset| {
        component_relationship_page(connection, source_scope, graph_version, limit, offset)
    })?;
    count += insert_relationship_batches(connection, |limit, offset| {
        sdk_relationship_page(connection, source_scope, graph_version, limit, offset)
    })?;
    count += insert_relationship_batches(connection, |limit, offset| {
        configuration_relationship_page(connection, source_scope, graph_version, limit, offset)
    })?;

    Ok(count)
}

pub(super) fn insert_file(
    connection: &Connection,
    file: &SoftwareFile,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_files (
            software_file_id, repository_id, source_scope, path, language_id, file_role,
            parse_status, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            file.software_file_id,
            file.repository_id,
            file.source_scope,
            file.path,
            file.language_id,
            file.file_role,
            file.parse_status,
            file.created_graph_version.get(),
        ],
    )?;

    Ok(())
}

pub(super) fn insert_topic(
    connection: &Connection,
    topic: &SoftwareTopic,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_topics (
            topic_id, repository_id, source_scope, name, topic_kind, source_path,
            line_start, line_end, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
        params![
            topic.topic_id,
            topic.repository_id,
            topic.source_scope,
            topic.name,
            topic.topic_kind,
            topic.source_path,
            topic.line_range.start,
            topic.line_range.end,
            topic.created_graph_version.get(),
        ],
    )?;

    Ok(())
}

pub(super) fn insert_relationship(
    connection: &Connection,
    relationship: &SoftwareRelationship,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_relationships (
            relationship_id, repository_id, source_scope, relationship_kind, source_id,
            source_kind, target_id, target_kind, target_hint, resolution_state,
            confidence_basis_points, confidence_tier, evidence_path, evidence_line_start,
            evidence_line_end, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
        params![
            relationship.relationship_id,
            relationship.repository_id,
            relationship.source_scope,
            relationship.relationship_kind,
            relationship.source_id,
            relationship.source_kind,
            relationship.target_id,
            relationship.target_kind,
            relationship.target_hint,
            relationship.resolution_state,
            relationship.confidence_basis_points,
            relationship.confidence_tier,
            relationship.evidence_path,
            relationship.evidence_line_range.start,
            relationship.evidence_line_range.end,
            relationship.created_graph_version.get(),
        ],
    )?;

    Ok(())
}

fn insert_relationship_batches(
    connection: &Connection,
    mut load_page: impl FnMut(usize, usize) -> Result<Vec<SoftwareRelationship>, StorageError>,
) -> Result<usize, StorageError> {
    let mut offset = 0;
    let mut count = 0;
    loop {
        let relationships = load_page(PROJECTION_PAGE_SIZE, offset)?;
        if relationships.is_empty() {
            break;
        }
        for relationship in &relationships {
            insert_relationship(connection, relationship)?;
        }
        let page_len = relationships.len();
        count += page_len;
        offset += page_len;
    }

    Ok(count)
}

fn document_relationship_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    offset: usize,
) -> Result<Vec<SoftwareRelationship>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT topics.repository_id, topics.source_scope, files.software_file_id,
               topics.topic_id, topics.name, topics.source_path, topics.line_start,
               topics.line_end
        FROM software_topics topics
        JOIN software_files files
          ON files.source_scope = topics.source_scope
         AND files.path = topics.source_path
        WHERE topics.source_scope = ?1
        ORDER BY topics.topic_kind ASC, topics.source_path ASC, topics.line_start ASC
        LIMIT ?2 OFFSET ?3
        ",
    )?;
    let rows = statement.query_map(params![source_scope, limit as i64, offset as i64], |row| {
        Ok(SoftwareRelationshipInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            relationship_kind: "documents".to_owned(),
            source_id: row.get(2)?,
            source_kind: "file".to_owned(),
            target_id: row.get(3)?,
            target_kind: "topic".to_owned(),
            target_hint: Some(row.get(4)?),
            resolution_state: "resolved".to_owned(),
            confidence_basis_points: 10_000,
            confidence_tier: "extracted".to_owned(),
            evidence_path: row.get(5)?,
            evidence_line_range: RepositoryCodeRange {
                start: row.get(6)?,
                end: row.get(7)?,
            },
            created_graph_version: graph_version,
        })
    })?;

    relationship_rows(rows)
}

fn component_relationship_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    offset: usize,
) -> Result<Vec<SoftwareRelationship>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT components.repository_id, components.source_scope, files.software_file_id,
               components.component_id, components.name, components.relationship_state,
               components.confidence_basis_points, components.evidence_path,
               components.evidence_line_start, components.evidence_line_end
        FROM software_components components
        JOIN software_files files
          ON files.source_scope = components.source_scope
         AND files.path = components.evidence_path
        WHERE components.source_scope = ?1
        ORDER BY components.evidence_path ASC, components.evidence_line_start ASC
        LIMIT ?2 OFFSET ?3
        ",
    )?;
    let rows = statement.query_map(params![source_scope, limit as i64, offset as i64], |row| {
        Ok(SoftwareRelationshipInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            relationship_kind: "depends_on".to_owned(),
            source_id: row.get(2)?,
            source_kind: "file".to_owned(),
            target_id: row.get(3)?,
            target_kind: "component".to_owned(),
            target_hint: Some(row.get(4)?),
            resolution_state: row.get(5)?,
            confidence_basis_points: row.get(6)?,
            confidence_tier: "extracted".to_owned(),
            evidence_path: row.get(7)?,
            evidence_line_range: RepositoryCodeRange {
                start: row.get(8)?,
                end: row.get(9)?,
            },
            created_graph_version: graph_version,
        })
    })?;

    relationship_rows(rows)
}

fn sdk_relationship_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    offset: usize,
) -> Result<Vec<SoftwareRelationship>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT usages.repository_id, usages.source_scope, files.software_file_id,
               usages.usage_id, usages.target_hint, usages.module, usages.resolution_state,
               usages.confidence_basis_points, usages.evidence_path,
               usages.evidence_line_start, usages.evidence_line_end
        FROM software_sdk_usages usages
        JOIN software_files files
          ON files.source_scope = usages.source_scope
         AND files.path = usages.evidence_path
        WHERE usages.source_scope = ?1
        ORDER BY usages.evidence_path ASC, usages.evidence_line_start ASC
        LIMIT ?2 OFFSET ?3
        ",
    )?;
    let rows = statement.query_map(params![source_scope, limit as i64, offset as i64], |row| {
        let target_hint = row.get::<_, Option<String>>(4)?;
        let module = row.get::<_, String>(5)?;
        Ok(SoftwareRelationshipInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            relationship_kind: "uses_sdk".to_owned(),
            source_id: row.get(2)?,
            source_kind: "file".to_owned(),
            target_id: row.get(3)?,
            target_kind: "sdk_usage".to_owned(),
            target_hint: target_hint.or(Some(module)),
            resolution_state: row.get(6)?,
            confidence_basis_points: row.get(7)?,
            confidence_tier: "ambiguous".to_owned(),
            evidence_path: row.get(8)?,
            evidence_line_range: RepositoryCodeRange {
                start: row.get(9)?,
                end: row.get(10)?,
            },
            created_graph_version: graph_version,
        })
    })?;

    relationship_rows(rows)
}

fn configuration_relationship_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    offset: usize,
) -> Result<Vec<SoftwareRelationship>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT flags.repository_id, flags.source_scope, files.software_file_id,
               flags.feature_flag_id, flags.path, flags.source_key, flags.edge_kind,
               flags.confidence_basis_points, flags.confidence_tier,
               flags.line_start, flags.line_end
        FROM code_repository_feature_flags flags
        JOIN software_files files
          ON files.source_scope = flags.source_scope
         AND files.path = flags.path
        WHERE flags.source_scope = ?1
        ORDER BY flags.path ASC, flags.line_start ASC
        LIMIT ?2 OFFSET ?3
        ",
    )?;
    let rows = statement.query_map(params![source_scope, limit as i64, offset as i64], |row| {
        let source_key = row.get::<_, String>(5)?;
        let relationship_kind = match row.get::<_, String>(6)?.as_str() {
            "defines_config" | "reads_config" | "guards_code" => "configures",
            _ => "references",
        };
        Ok(SoftwareRelationshipInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            relationship_kind: relationship_kind.to_owned(),
            source_id: row.get(2)?,
            source_kind: "file".to_owned(),
            target_id: row.get(3)?,
            target_kind: "configuration".to_owned(),
            target_hint: Some(source_key),
            resolution_state: "inferred".to_owned(),
            confidence_basis_points: row.get(7)?,
            confidence_tier: row.get(8)?,
            evidence_path: row.get(4)?,
            evidence_line_range: RepositoryCodeRange {
                start: row.get(9)?,
                end: row.get(10)?,
            },
            created_graph_version: graph_version,
        })
    })?;

    relationship_rows(rows)
}

fn relationship_rows<F>(
    rows: rusqlite::MappedRows<'_, F>,
) -> Result<Vec<SoftwareRelationship>, StorageError>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<SoftwareRelationshipInput>,
{
    rows.map(|row| row.map_err(StorageError::from).and_then(relationship))
        .collect()
}

pub(super) fn files_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareFile>, StorageError> {
    let path_filter = super::path_filter_sql_for_column("path", &request.repository.path_filters);
    let language_filter =
        super::language_filter_sql_for_column("language_id", &request.repository.language_filters);
    let query = format!(
        "
        SELECT software_file_id, repository_id, source_scope, path, language_id, file_role,
               parse_status, created_graph_version
        FROM software_files
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY
            CASE file_role
                WHEN 'dependency_manifest' THEN 0
                WHEN 'build_manifest' THEN 1
                WHEN 'source' THEN 2
                WHEN 'documentation' THEN 3
                WHEN 'configuration' THEN 4
                WHEN 'deployment' THEN 5
                WHEN 'test' THEN 6
                WHEN 'template' THEN 7
                WHEN 'knowledge_map' THEN 8
                ELSE 9
            END ASC,
            CASE
                WHEN path = 'Cargo.toml' OR path LIKE '%/Cargo.toml' THEN 0
                WHEN path = 'package.json' OR path LIKE '%/package.json' THEN 1
                WHEN path = 'pyproject.toml' OR path LIKE '%/pyproject.toml' THEN 2
                WHEN path = 'go.mod' OR path LIKE '%/go.mod' THEN 3
                WHEN path = 'pom.xml' OR path LIKE '%/pom.xml' THEN 4
                WHEN path = 'build.gradle' OR path LIKE '%/build.gradle'
                  OR path = 'build.gradle.kts' OR path LIKE '%/build.gradle.kts' THEN 5
                WHEN path = 'CMakeLists.txt' OR path LIKE '%/CMakeLists.txt' THEN 6
                WHEN path = 'Makefile' OR path LIKE '%/Makefile' THEN 7
                WHEN path = 'Cargo.lock' OR path LIKE '%/Cargo.lock'
                  OR path = 'package-lock.json' OR path LIKE '%/package-lock.json'
                  OR path = 'go.sum' OR path LIKE '%/go.sum'
                  OR path = 'uv.lock' OR path LIKE '%/uv.lock'
                  OR path = 'gradle.lockfile' OR path LIKE '%/gradle.lockfile' THEN 20
                ELSE 10
            END ASC,
            path ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), file_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn topics_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareTopic>, StorageError> {
    let path_filter =
        super::path_filter_sql_for_column("topics.source_path", &request.repository.path_filters);
    let language_filter = super::language_filter_sql_for_column(
        "files.language_id",
        &request.repository.language_filters,
    );
    let query = format!(
        "
        SELECT topics.topic_id, topics.repository_id, topics.source_scope, topics.name,
               topics.topic_kind, topics.source_path, topics.line_start, topics.line_end,
               topics.created_graph_version
        FROM software_topics topics
        JOIN software_files files
          ON files.source_scope = topics.source_scope
         AND files.path = topics.source_path
        WHERE topics.source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY topics.topic_kind ASC, topics.source_path ASC, topics.line_start ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), topic_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn relationships_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareRelationship>, StorageError> {
    let path_filter = super::path_filter_sql_for_column(
        "relationships.evidence_path",
        &request.repository.path_filters,
    );
    let language_filter = relationship_language_filter_sql(&request.repository.language_filters);
    let query = format!(
        "
        SELECT relationships.relationship_id, relationships.repository_id,
               relationships.source_scope, relationships.relationship_kind,
               relationships.source_id, relationships.source_kind, relationships.target_id,
               relationships.target_kind, relationships.target_hint,
               relationships.resolution_state, relationships.confidence_basis_points,
               relationships.confidence_tier, relationships.evidence_path,
               relationships.evidence_line_start, relationships.evidence_line_end,
               relationships.created_graph_version
        FROM software_relationships relationships
        JOIN software_files files
          ON files.source_scope = relationships.source_scope
         AND files.path = relationships.evidence_path
        LEFT JOIN software_components components
          ON components.source_scope = relationships.source_scope
         AND components.component_id = relationships.target_id
         AND relationships.relationship_kind = 'depends_on'
        WHERE relationships.source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY
            CASE relationships.relationship_kind
                WHEN 'depends_on' THEN 0
                WHEN 'uses_sdk' THEN 1
                WHEN 'documents' THEN 2
                WHEN 'configures' THEN 3
                ELSE 4
            END ASC,
            CASE relationships.resolution_state
                WHEN 'declared' THEN 0
                WHEN 'resolved' THEN 1
                WHEN 'extracted' THEN 2
                WHEN 'inferred' THEN 3
                WHEN 'locked' THEN 4
                ELSE 5
            END ASC,
            CASE files.file_role
                WHEN 'dependency_manifest' THEN 0
                WHEN 'build_manifest' THEN 1
                WHEN 'deployment' THEN 2
                WHEN 'source' THEN 3
                WHEN 'configuration' THEN 4
                WHEN 'documentation' THEN 5
                ELSE 6
            END ASC,
            relationships.confidence_basis_points DESC,
            relationships.evidence_path ASC,
            relationships.evidence_line_start ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    super::push_path_filter_values(&mut values, &request.repository.path_filters);
    push_relationship_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), relationship_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn count_topics(connection: &Connection, source_scope: &str) -> Result<usize, StorageError> {
    let count = connection.query_row(
        "
        SELECT COUNT(*)
        FROM software_topics
        WHERE source_scope = ?1
        ",
        params![source_scope],
        |row| row.get::<_, usize>(0),
    )?;

    Ok(count)
}

fn markdown_heading_topic_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    offset: usize,
) -> Result<Vec<SoftwareTopic>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT repository_id, source_scope, name, path, line_start, line_end
        FROM code_repository_symbols
        WHERE source_scope = ?1
          AND language_id = 'markdown'
          AND kind = 'heading'
        ORDER BY path ASC, line_start ASC
        LIMIT ?2 OFFSET ?3
        ",
    )?;
    let rows = statement.query_map(params![source_scope, limit as i64, offset as i64], |row| {
        Ok(SoftwareTopicInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            name: row.get(2)?,
            topic_kind: "document_heading".to_owned(),
            source_path: row.get(3)?,
            line_range: RepositoryCodeRange {
                start: row.get(4)?,
                end: row.get(5)?,
            },
            created_graph_version: graph_version,
        })
    })?;

    rows.map(|row| {
        row.map_err(StorageError::from).and_then(|input| {
            SoftwareTopic::new(input).map_err(|error| StorageError::InvalidInput(error.to_string()))
        })
    })
    .collect()
}

fn knowledge_map_topic_page(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
    offset: usize,
) -> Result<Vec<SoftwareTopic>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT repository_id, source_scope, path, name, line_start, line_end
        FROM code_repository_symbols
        WHERE source_scope = ?1
          AND path = ?2
          AND kind = 'knowledge_map_topic'
        ORDER BY line_start ASC
        LIMIT ?3 OFFSET ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            source_scope,
            KNOWLEDGE_MAP_RELATIVE_PATH,
            limit as i64,
            offset as i64
        ],
        |row| {
            Ok(SoftwareTopicInput {
                repository_id: row.get(0)?,
                source_scope: row.get(1)?,
                source_path: row.get(2)?,
                name: row.get(3)?,
                topic_kind: "knowledge_map_topic".to_owned(),
                line_range: RepositoryCodeRange {
                    start: row.get(4)?,
                    end: row.get(5)?,
                },
                created_graph_version: graph_version,
            })
        },
    )?;
    let mut topics = Vec::new();
    for row in rows {
        let input = row?;
        topics.push(
            SoftwareTopic::new(input)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
        );
    }

    Ok(topics)
}

fn relationship(input: SoftwareRelationshipInput) -> Result<SoftwareRelationship, StorageError> {
    SoftwareRelationship::new(input).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn relationship_language_filter_sql(filters: &[String]) -> String {
    let clauses = filters
        .iter()
        .map(|_| {
            "(files.language_id = ? OR \
             (relationships.relationship_kind = 'depends_on' AND components.language_id = ?))"
        })
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

fn push_relationship_language_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    for filter in filters {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(filter.clone()));
    }
}

fn file_role(path: &str, language_id: &str) -> &'static str {
    if path == KNOWLEDGE_MAP_RELATIVE_PATH {
        return "knowledge_map";
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if language_id == "markdown" {
        return "documentation";
    }
    if dependency_manifest_path(path, file_name) {
        return "dependency_manifest";
    }
    if build_manifest_path(file_name, language_id) {
        return "build_manifest";
    }
    if deployment_path(path, file_name, language_id) {
        return "deployment";
    }
    if test_path(path, file_name) {
        return "test";
    }
    if template_language(language_id) {
        return "template";
    }
    if config_language(language_id) {
        return "configuration";
    }

    "source"
}

fn dependency_manifest_path(path: &str, file_name: &str) -> bool {
    matches!(
        file_name,
        "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "go.mod"
            | "go.sum"
            | "requirements.txt"
            | "pyproject.toml"
            | "uv.lock"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "gradle.lockfile"
            | "conanfile.txt"
            | "conanfile.py"
            | "CMakeLists.txt"
    ) || python_requirements_path(path, file_name)
}

fn python_requirements_path(path: &str, file_name: &str) -> bool {
    file_name.ends_with(".txt")
        && (file_name.starts_with("requirements")
            || file_name.starts_with("constraints")
            || path.split('/').any(|segment| segment == "requirements"))
}

fn build_manifest_path(file_name: &str, language_id: &str) -> bool {
    matches!(
        file_name,
        "BUILD"
            | "BUILD.bazel"
            | "WORKSPACE"
            | "WORKSPACE.bazel"
            | "MODULE.bazel"
            | "Makefile"
            | "GNUmakefile"
            | "BSDmakefile"
            | "CMakeLists.txt"
            | "build.ninja"
    ) || matches!(language_id, "cmake" | "make" | "ninja" | "starlark")
}

fn deployment_path(path: &str, file_name: &str, language_id: &str) -> bool {
    file_name.starts_with("Dockerfile")
        || file_name.starts_with("Containerfile")
        || matches!(language_id, "dockerfile")
        || (deployment_service_path(path)
            && (deployment_manifest_language(language_id) || service_manager_file_name(file_name)))
        || (kubernetes_manifest_path(path) && deployment_manifest_language(language_id))
}

fn deployment_service_path(path: &str) -> bool {
    path.starts_with("systemd/")
        || path.starts_with("launchd/")
        || path.contains("/systemd/")
        || path.contains("/launchd/")
}

fn kubernetes_manifest_path(path: &str) -> bool {
    path.starts_with("k8s/")
        || path.starts_with("kubernetes/")
        || path.contains("/k8s/")
        || path.contains("/kubernetes/")
}

fn deployment_manifest_language(language_id: &str) -> bool {
    config_language(language_id) || template_language(language_id)
}

fn service_manager_file_name(file_name: &str) -> bool {
    file_name.ends_with(".service")
        || file_name.ends_with(".socket")
        || file_name.ends_with(".timer")
        || file_name.ends_with(".target")
        || file_name.ends_with(".plist")
}

fn test_path(path: &str, file_name: &str) -> bool {
    path.starts_with("test/")
        || path.starts_with("tests/")
        || path.contains("/test/")
        || path.contains("/tests/")
        || file_name.contains("_test.")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
}

fn template_language(language_id: &str) -> bool {
    matches!(language_id, "jinja2" | "gotemplate")
}

fn config_language(language_id: &str) -> bool {
    matches!(
        language_id,
        "json" | "toml" | "yaml" | "ini" | "properties" | "xml" | "jinja2" | "gotemplate"
    )
}

fn file_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareFile> {
    Ok(SoftwareFile {
        software_file_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        path: row.get(3)?,
        language_id: row.get(4)?,
        file_role: row.get(5)?,
        parse_status: row.get(6)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(7)?),
    })
}

fn topic_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareTopic> {
    Ok(SoftwareTopic {
        topic_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        name: row.get(3)?,
        topic_kind: row.get(4)?,
        source_path: row.get(5)?,
        line_range: RepositoryCodeRange {
            start: row.get(6)?,
            end: row.get(7)?,
        },
        created_graph_version: GraphVersion::new(row.get::<_, u64>(8)?),
    })
}

fn relationship_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareRelationship> {
    Ok(SoftwareRelationship {
        relationship_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        relationship_kind: row.get(3)?,
        source_id: row.get(4)?,
        source_kind: row.get(5)?,
        target_id: row.get(6)?,
        target_kind: row.get(7)?,
        target_hint: row.get(8)?,
        resolution_state: row.get(9)?,
        confidence_basis_points: row.get(10)?,
        confidence_tier: row.get(11)?,
        evidence_path: row.get(12)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(13)?,
            end: row.get(14)?,
        },
        created_graph_version: GraphVersion::new(row.get::<_, u64>(15)?),
    })
}
