use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    domain::{CodeDependencyRecord, GraphVersion, RepositoryCodeRange, SoftwareBuildTargetInput},
    storage::StorageError,
};

use super::{code::SearchDocumentInserter, helpers::stable_id};

#[path = "maven/model.rs"]
mod model;
#[cfg(test)]
#[path = "maven/review_followup_tests.rs"]
mod review_followup_tests;
#[cfg(test)]
#[path = "maven/review_tests.rs"]
mod review_tests;
#[path = "maven/support.rs"]
mod support;
#[cfg(test)]
#[path = "maven/tests.rs"]
mod tests;
#[path = "maven/xml.rs"]
mod xml;

use model::{EffectivePom, JVM_LANGUAGES, PomDocument, resolve_effective_model_load};

const MAVEN_SOURCE_KIND: &str = "pom.xml";
const FILE_CHUNK_CONTENT_BUDGET_BYTES: u64 = 8_000;

#[derive(Debug)]
struct ChunkSchema {
    has_file_id: bool,
    has_symbol_snapshot_id: bool,
    has_byte_start: bool,
    has_byte_end: bool,
}

#[derive(Debug)]
struct PomLoad {
    documents: Vec<PomDocument>,
    has_truncated_documents: bool,
}

#[derive(Debug)]
struct MavenModels {
    models: Vec<EffectivePom>,
    preserve_existing_facts: bool,
}

struct BuildFactEvidence<'a> {
    path: &'a str,
    line: u32,
}

#[derive(Debug, Clone)]
pub(super) struct MavenBuildFact {
    repository_id: String,
    source_scope: String,
    path: String,
    language_id: String,
    name: String,
    kind: String,
    command: Option<String>,
    output_hint: Option<String>,
    line: u32,
}

pub(super) fn build_target_inputs(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<Vec<SoftwareBuildTargetInput>, StorageError> {
    let loaded = effective_models(connection, source_scope)?;
    if loaded.preserve_existing_facts {
        return Ok(Vec::new());
    }
    Ok(loaded
        .models
        .into_iter()
        .flat_map(|model| build_facts(&model))
        .map(|fact| build_input(fact, graph_version))
        .collect())
}

pub(super) fn preserves_existing_facts(
    connection: &Connection,
    source_scope: &str,
) -> Result<bool, StorageError> {
    Ok(effective_models(connection, source_scope)?.preserve_existing_facts)
}

#[cfg(test)]
pub(super) fn refresh_effective_dependencies(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    let loaded = effective_models(transaction, source_scope)?;
    refresh_effective_dependency_records(transaction, source_scope, loaded)
}

pub(super) fn refresh_effective_dependencies_with_language_filters(
    transaction: &Transaction<'_>,
    source_scope: &str,
    language_filters: &[String],
) -> Result<(), StorageError> {
    let languages = jvm_languages_for_filters(language_filters);
    let loaded = effective_models_with_languages(transaction, source_scope, languages)?;
    refresh_effective_dependency_records(transaction, source_scope, loaded)
}

fn refresh_effective_dependency_records(
    transaction: &Transaction<'_>,
    source_scope: &str,
    loaded: MavenModels,
) -> Result<(), StorageError> {
    if loaded.preserve_existing_facts {
        return Ok(());
    }
    transaction.execute(
        "
        DELETE FROM code_repository_search
        WHERE rowid IN (
            SELECT search_rowid
            FROM code_repository_search_metadata
            WHERE source_scope = ?1
              AND document_kind = 'dependency'
              AND record_id IN (
                  SELECT dependency_id
                  FROM code_repository_dependencies
                  WHERE source_scope = ?1
                    AND ecosystem = 'maven'
                    AND source_kind = 'pom.xml'
              )
        )
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        DELETE FROM code_repository_search_metadata
        WHERE source_scope = ?1
          AND document_kind = 'dependency'
          AND record_id IN (
              SELECT dependency_id
              FROM code_repository_dependencies
              WHERE source_scope = ?1
                AND ecosystem = 'maven'
                AND source_kind = 'pom.xml'
          )
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "
        DELETE FROM code_repository_dependencies
        WHERE source_scope = ?1
          AND ecosystem = 'maven'
          AND source_kind = 'pom.xml'
        ",
        params![source_scope],
    )?;
    if loaded.models.is_empty() {
        return Ok(());
    }

    let mut insert_dependency = transaction.prepare(
        "
        INSERT INTO code_repository_dependencies (
            repository_id, source_scope, dependency_id, file_id, path, language_id,
            ecosystem, package_name, requirement, resolved_version, dependency_group,
            source_kind, is_lockfile, line_start, line_end, excerpt
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for record in dependency_records(&loaded.models) {
        insert_dependency.execute(params![
            record.repository_id,
            record.source_scope,
            record.dependency_id,
            record.file_id,
            record.path,
            record.language_id,
            record.ecosystem,
            record.package_name,
            record.requirement,
            record.resolved_version,
            record.dependency_group,
            record.source_kind,
            record.is_lockfile,
            record.line_range.start,
            record.line_range.end,
            record.excerpt,
        ])?;
        search_documents.insert(
            &record.source_scope,
            "dependency",
            &record.dependency_id,
            &record.path,
            &record.language_id,
            [
                record.ecosystem.as_str(),
                record.package_name.as_str(),
                record.requirement.as_deref().unwrap_or_default(),
                record.resolved_version.as_deref().unwrap_or_default(),
                record.dependency_group.as_str(),
                record.source_kind.as_str(),
                record.excerpt.as_str(),
                record.path.as_str(),
            ],
        )?;
    }

    Ok(())
}

fn effective_models(
    connection: &Connection,
    source_scope: &str,
) -> Result<MavenModels, StorageError> {
    let languages = scope_jvm_languages(connection, source_scope)?;
    effective_models_with_languages(connection, source_scope, languages)
}

fn effective_models_with_languages(
    connection: &Connection,
    source_scope: &str,
    languages: Vec<&'static str>,
) -> Result<MavenModels, StorageError> {
    let loaded = pom_documents(connection, source_scope)?;
    if loaded.documents.is_empty() {
        return Ok(MavenModels {
            models: Vec::new(),
            preserve_existing_facts: loaded.has_truncated_documents,
        });
    }

    let loaded_models = resolve_effective_model_load(loaded.documents)?;
    let mut models = loaded_models.models;
    for model in &mut models {
        model.languages.clone_from(&languages);
    }
    Ok(MavenModels {
        models,
        preserve_existing_facts: loaded.has_truncated_documents
            || loaded_models.preserve_existing_facts,
    })
}

fn pom_documents(connection: &Connection, source_scope: &str) -> Result<PomLoad, StorageError> {
    let chunk_schema = read_chunk_schema(connection)?;
    let file_id_expression = if chunk_schema.has_file_id {
        "file_id"
    } else {
        "path"
    };
    let byte_start_expression = if chunk_schema.has_byte_start {
        "byte_start"
    } else {
        "0"
    };
    let byte_end_expression = if chunk_schema.has_byte_end {
        "byte_end"
    } else {
        "LENGTH(content)"
    };
    let symbol_filter = if chunk_schema.has_symbol_snapshot_id {
        "AND symbol_snapshot_id IS NULL"
    } else {
        ""
    };
    let query = format!(
        "
        SELECT repository_id, source_scope, {file_id_expression}, path, content,
               {byte_start_expression}, {byte_end_expression}
        FROM code_repository_chunks
        WHERE source_scope = ?1
          AND (path = 'pom.xml' OR path LIKE '%/pom.xml')
          {symbol_filter}
        ORDER BY path ASC, line_start ASC, chunk_id ASC
        ",
    );
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(PomDocument {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            file_id: row.get(2)?,
            path: row.get(3)?,
            content: row.get(4)?,
            byte_start: row.get::<_, u64>(5)?,
            byte_end: row.get::<_, u64>(6)?,
        })
    })?;

    let mut documents = Vec::new();
    let mut has_truncated_documents = false;
    for row in rows {
        let document = row?;
        if document_is_truncated(&document) {
            has_truncated_documents = true;
            continue;
        }
        documents.push(document);
    }
    Ok(PomLoad {
        documents,
        has_truncated_documents,
    })
}

fn read_chunk_schema(connection: &Connection) -> Result<ChunkSchema, StorageError> {
    let mut statement = connection.prepare("PRAGMA table_info(code_repository_chunks)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut column_names = BTreeSet::new();
    for row in rows {
        column_names.insert(row?);
    }
    Ok(ChunkSchema {
        has_file_id: column_names.contains("file_id"),
        has_symbol_snapshot_id: column_names.contains("symbol_snapshot_id"),
        has_byte_start: column_names.contains("byte_start"),
        has_byte_end: column_names.contains("byte_end"),
    })
}

fn document_is_truncated(document: &PomDocument) -> bool {
    let source_span = document.byte_end.saturating_sub(document.byte_start);
    source_span > FILE_CHUNK_CONTENT_BUDGET_BYTES && (document.content.len() as u64) < source_span
}

fn scope_jvm_languages(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<&'static str>, StorageError> {
    let filters_json = connection
        .query_row(
            "
            SELECT language_filters_json
            FROM code_repository_scopes
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(filters_json) = filters_json else {
        return Ok(JVM_LANGUAGES.to_vec());
    };
    let filters = serde_json::from_str::<Vec<String>>(&filters_json)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    Ok(jvm_languages_for_filters(&filters))
}

fn jvm_languages_for_filters(filters: &[String]) -> Vec<&'static str> {
    if filters.is_empty() {
        return JVM_LANGUAGES.to_vec();
    }

    JVM_LANGUAGES
        .into_iter()
        .filter(|language| {
            filters
                .iter()
                .any(|filter| filter.eq_ignore_ascii_case(language))
        })
        .collect()
}

fn build_facts(model: &EffectivePom) -> Vec<MavenBuildFact> {
    let mut facts = Vec::new();
    for language_id in model.languages.iter().copied() {
        facts.push(build_fact(
            model,
            language_id,
            "project",
            model.coordinate.as_str(),
            Some(format!("mvn {}", model.packaging_phase())),
            model.packaging.clone(),
            model.line,
        ));
        facts.push(build_fact(
            model,
            language_id,
            "package",
            model.artifact_id.as_str(),
            Some(format!("mvn {}", model.packaging_phase())),
            model.packaging.clone(),
            model.line,
        ));
        if let Some(packaging) = &model.packaging {
            facts.push(build_fact(
                model,
                language_id,
                "packaging",
                packaging,
                Some(format!("mvn {}", model.packaging_phase())),
                None,
                model.line,
            ));
        }
        for module in &model.modules {
            facts.push(build_fact(
                model,
                language_id,
                "module",
                module.value.as_str(),
                Some(format!("mvn -pl {} package", module.value)),
                None,
                module.line,
            ));
        }
        for profile in &model.profiles {
            facts.push(build_fact(
                model,
                language_id,
                "profile",
                profile.id.as_str(),
                Some(format!("mvn -P{} {}", profile.id, model.packaging_phase())),
                None,
                profile.line,
            ));
        }
        for plugin in &model.plugins {
            let plugin_help = format!("{}:help", plugin.prefix());
            let plugin_name = plugin.scoped_name(plugin.coordinate.as_str());
            facts.push(build_fact_for_evidence(
                model,
                BuildFactEvidence {
                    path: plugin.source_path.as_str(),
                    line: plugin.line,
                },
                language_id,
                "plugin",
                plugin_name.as_str(),
                Some(plugin.command(&plugin_help)),
                plugin.version.clone(),
            ));
            for execution in &plugin.executions {
                let name =
                    plugin.scoped_name(&format!("{}:{}", plugin.coordinate, execution.name()));
                facts.push(build_fact_for_evidence(
                    model,
                    BuildFactEvidence {
                        path: execution.source_path.as_str(),
                        line: execution.line,
                    },
                    language_id,
                    "execution",
                    name.as_str(),
                    execution.command(plugin),
                    execution.phase.clone(),
                ));
                for goal in &execution.goals {
                    let goal_target = format!("{}:{}", plugin.prefix(), goal.value);
                    let goal_name = plugin.scoped_name(&goal_target);
                    facts.push(build_fact_for_evidence(
                        model,
                        BuildFactEvidence {
                            path: goal.source_path.as_str(),
                            line: goal.line,
                        },
                        language_id,
                        "goal",
                        goal_name.as_str(),
                        Some(plugin.command(&goal_target)),
                        execution.phase.clone(),
                    ));
                }
            }
        }
    }

    facts
}

fn build_fact(
    model: &EffectivePom,
    language_id: &str,
    kind: &str,
    name: &str,
    command: Option<String>,
    output_hint: Option<String>,
    line: u32,
) -> MavenBuildFact {
    build_fact_for_evidence(
        model,
        BuildFactEvidence {
            path: model.document.path.as_str(),
            line,
        },
        language_id,
        kind,
        name,
        command,
        output_hint,
    )
}

fn build_fact_for_evidence(
    model: &EffectivePom,
    evidence: BuildFactEvidence<'_>,
    language_id: &str,
    kind: &str,
    name: &str,
    command: Option<String>,
    output_hint: Option<String>,
) -> MavenBuildFact {
    MavenBuildFact {
        repository_id: model.document.repository_id.clone(),
        source_scope: model.document.source_scope.clone(),
        path: evidence.path.to_owned(),
        language_id: language_id.to_owned(),
        name: name.to_owned(),
        kind: kind.to_owned(),
        command,
        output_hint,
        line: evidence.line,
    }
}

fn dependency_records(models: &[EffectivePom]) -> Vec<CodeDependencyRecord> {
    let mut seen = BTreeSet::new();
    let mut records = Vec::new();
    for model in models {
        for dependency in &model.dependencies {
            for &language_id in &model.languages {
                let line = dependency.line.max(1);
                let package_name = dependency.coordinate();
                let dependency_group = dependency.dependency_group();
                let source_path = dependency.source_path.as_str();
                let source_file_id = dependency.source_file_id.as_str();
                let key = format!(
                    "{}\0{}\0{}\0{}\0{}\0{}",
                    model.document.source_scope,
                    source_path,
                    language_id,
                    package_name,
                    dependency_group,
                    line
                );
                if !seen.insert(key.clone()) {
                    continue;
                }
                records.push(CodeDependencyRecord {
                    repository_id: model.document.repository_id.clone(),
                    source_scope: model.document.source_scope.clone(),
                    dependency_id: stable_id("dependency", &key),
                    file_id: source_file_id.to_owned(),
                    path: source_path.to_owned(),
                    language_id: language_id.to_owned(),
                    ecosystem: "maven".to_owned(),
                    package_name: package_name.clone(),
                    requirement: dependency.version.clone(),
                    resolved_version: None,
                    dependency_group,
                    source_kind: MAVEN_SOURCE_KIND.to_owned(),
                    is_lockfile: false,
                    line_range: RepositoryCodeRange {
                        start: line,
                        end: line,
                    },
                    excerpt: dependency.excerpt(&package_name),
                });
            }
        }
    }

    records
}

fn build_input(fact: MavenBuildFact, graph_version: GraphVersion) -> SoftwareBuildTargetInput {
    SoftwareBuildTargetInput {
        repository_id: fact.repository_id,
        source_scope: fact.source_scope,
        ecosystem: "maven".to_owned(),
        language_id: fact.language_id,
        name: fact.name,
        kind: fact.kind,
        command: fact.command,
        output_hint: fact.output_hint,
        source_kind: MAVEN_SOURCE_KIND.to_owned(),
        evidence_path: fact.path,
        evidence_line_range: RepositoryCodeRange {
            start: fact.line,
            end: fact.line,
        },
        confidence_basis_points: 9_000,
        created_graph_version: graph_version,
    }
}
