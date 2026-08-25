use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareGlobalRequest, SoftwareTopic, SoftwareTopicInput,
    },
    project::KNOWLEDGE_MAP_RELATIVE_PATH,
    storage::StorageError,
};

const TOPIC_PROJECTION_PAGE_SIZE: usize = 512;

pub(in crate::storage::sqlite::software) fn materialize_topics(
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
            TOPIC_PROJECTION_PAGE_SIZE,
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
            TOPIC_PROJECTION_PAGE_SIZE,
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

fn insert_topic(connection: &Connection, topic: &SoftwareTopic) -> Result<(), StorageError> {
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

pub(in crate::storage::sqlite::software) fn topics_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareTopic>, StorageError> {
    let path_filter = super::super::path_filter_sql_for_column(
        "topics.source_path",
        &request.repository.path_filters,
    );
    let language_filter = super::super::language_filter_sql_for_column(
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
    super::super::push_path_filter_values(&mut values, &request.repository.path_filters);
    super::super::push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), topic_from_row)?;

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
        FROM (
            SELECT legacy.repository_id, legacy.source_scope, legacy.path, legacy.name,
                   legacy.line_start, legacy.line_end
            FROM code_repository_symbols legacy
            WHERE legacy.source_scope = ?1
              AND legacy.path = ?2
              AND legacy.kind = 'knowledge_map_topic'
            UNION ALL
            SELECT shards.repository_id, shards.source_scope, shards.path, shards.name,
                   shards.line_start, shards.line_end
            FROM code_repository_symbols refs
            JOIN code_repository_symbols topics
              ON topics.repository_id = refs.repository_id
             AND topics.source_scope = refs.source_scope
             AND topics.path = refs.path
             AND topics.line_start = refs.line_start
             AND topics.kind = 'knowledge_map_topic_shard_topic'
            JOIN code_repository_symbols root_identity
              ON root_identity.repository_id = refs.repository_id
             AND root_identity.source_scope = refs.source_scope
             AND root_identity.path = refs.path
             AND root_identity.line_start = refs.line_start
             AND root_identity.kind = 'knowledge_map_topic_shard_identity'
            JOIN code_repository_symbols shards
              ON shards.repository_id = refs.repository_id
             AND shards.source_scope = refs.source_scope
             AND shards.path = refs.name
             AND shards.name = topics.name
             AND shards.kind = 'knowledge_map_topic_shard'
            JOIN code_repository_symbols shard_identity
              ON shard_identity.repository_id = shards.repository_id
             AND shard_identity.source_scope = shards.source_scope
             AND shard_identity.path = shards.path
             AND shard_identity.line_start = shards.line_start
             AND shard_identity.kind = 'knowledge_map_topic_shard_identity'
             AND shard_identity.name = root_identity.name
            WHERE refs.source_scope = ?1
              AND refs.path = ?2
              AND refs.kind = 'knowledge_map_topic_shard_ref'
        ) current_topics
        ORDER BY path ASC, line_start ASC
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

#[cfg(test)]
#[path = "topics_tests.rs"]
mod tests;
