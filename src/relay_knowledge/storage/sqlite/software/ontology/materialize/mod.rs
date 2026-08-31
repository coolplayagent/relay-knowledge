use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, params};

use crate::{
    domain::{
        GraphVersion, SoftwareAssertionMode, SoftwareEntity, SoftwareEntityInput,
        SoftwareEntityKind, SoftwareEvidenceRef, SoftwareFactState, SoftwarePredicate,
        SoftwareShapeDiagnostic, SoftwareSourceCoverage, SoftwareSourceKind, SoftwareStatement,
        SoftwareStatementInput, SoftwareStatementResolution, reconcile_software_statements,
    },
    storage::StorageError,
};

mod code;
mod lifecycle;
mod repository;

const MAX_ONTOLOGY_ENTITIES_PER_SCOPE: usize = 262_144;
const MAX_ONTOLOGY_STATEMENTS_PER_SCOPE: usize = 524_288;
const EXTRACTOR_ID: &str = "relay-knowledge/software-ontology";
const EXTRACTOR_VERSION: &str = crate::domain::SOFTWARE_ONTOLOGY_VERSION;

pub(in crate::storage::sqlite::software) struct SoftwareOntologyProjection {
    pub(in crate::storage::sqlite::software) entities: Vec<SoftwareEntity>,
    pub(in crate::storage::sqlite::software) statements: Vec<SoftwareStatement>,
    pub(in crate::storage::sqlite::software) diagnostics: Vec<SoftwareShapeDiagnostic>,
    pub(in crate::storage::sqlite::software) source_coverage: SoftwareSourceCoverage,
    pub(in crate::storage::sqlite::software) completeness_basis_points: u16,
    pub(in crate::storage::sqlite::software) conflict_count: usize,
}

pub(in crate::storage::sqlite::software) fn refresh_projection(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<SoftwareOntologyProjection, StorageError> {
    super::schema::delete_scope(connection, source_scope)?;
    let repository_id =
        super::super::query_scope::repository_id_for_scope(connection, source_scope)?.ok_or_else(
            || {
                StorageError::Invariant(format!(
                    "repository identity for software ontology scope '{source_scope}' is missing"
                ))
            },
        )?;
    let mut builder = OntologyBuilder::new(repository_id, source_scope.to_owned(), graph_version)?;
    repository::collect_files(connection, &mut builder)?;
    repository::collect_components(connection, &mut builder)?;
    repository::collect_sdk_usages(connection, &mut builder)?;
    repository::collect_topics(connection, &mut builder)?;
    lifecycle::collect_build_targets(connection, &mut builder)?;
    lifecycle::collect_iac_resources(connection, &mut builder)?;
    lifecycle::collect_design_elements(connection, &mut builder)?;
    code::collect_api_and_test_symbols(connection, &mut builder)?;
    code::collect_configurations(connection, &mut builder)?;

    let (statements, report) = reconcile_software_statements(&builder.entities, builder.statements);
    let source_coverage = source_coverage(&statements);
    let completeness_basis_points = provenance_completeness(&statements);
    let conflict_count = conflict_count(&statements);
    persist_entities(connection, &builder.entities)?;
    persist_statements(connection, &statements)?;
    persist_diagnostics(connection, source_scope, &report.diagnostics)?;

    Ok(SoftwareOntologyProjection {
        entities: builder.entities,
        statements,
        diagnostics: report.diagnostics,
        source_coverage,
        completeness_basis_points,
        conflict_count,
    })
}

struct OntologyBuilder {
    repository_id: String,
    source_scope: String,
    graph_version: GraphVersion,
    snapshot_key: String,
    entities: Vec<SoftwareEntity>,
    statements: Vec<SoftwareStatement>,
    occurrence_ids: HashSet<String>,
    statement_ids: HashSet<String>,
    files_by_path: HashMap<String, String>,
    deployment_by_path: HashMap<String, String>,
}

struct OntologyEntityCandidate<'a> {
    projection_id: Option<&'a str>,
    kind: SoftwareEntityKind,
    name: String,
    namespace: Option<String>,
    source_kind: SoftwareSourceKind,
    evidence: SoftwareEvidenceRef,
    attributes: BTreeMap<String, String>,
}

impl OntologyBuilder {
    fn new(
        repository_id: String,
        source_scope: String,
        graph_version: GraphVersion,
    ) -> Result<Self, StorageError> {
        let snapshot = SoftwareEntity::new(SoftwareEntityInput {
            repository_id: repository_id.clone(),
            source_scope: source_scope.clone(),
            entity_kind: SoftwareEntityKind::RepositorySnapshot,
            name: source_scope.clone(),
            namespace: Some(repository_id.clone()),
            source_kind: SoftwareSourceKind::Code,
            evidence_refs: Vec::new(),
            attributes: BTreeMap::new(),
            created_graph_version: graph_version,
        })
        .map_err(domain_error)?;
        let snapshot_key = snapshot.entity_key.clone();
        let occurrence_id = snapshot.occurrence_id.clone();
        Ok(Self {
            repository_id,
            source_scope,
            graph_version,
            snapshot_key,
            entities: vec![snapshot],
            statements: Vec::new(),
            occurrence_ids: HashSet::from([occurrence_id]),
            statement_ids: HashSet::new(),
            files_by_path: HashMap::new(),
            deployment_by_path: HashMap::new(),
        })
    }

    fn evidence(
        &self,
        path: &str,
        start: u32,
        end: u32,
    ) -> Result<SoftwareEvidenceRef, StorageError> {
        SoftwareEvidenceRef::new(
            self.source_scope.clone(),
            path.to_owned(),
            crate::domain::RepositoryCodeRange { start, end },
        )
        .map_err(domain_error)
    }

    fn add_entity(
        &mut self,
        candidate: OntologyEntityCandidate<'_>,
    ) -> Result<String, StorageError> {
        let OntologyEntityCandidate {
            projection_id,
            kind,
            name,
            namespace,
            source_kind,
            evidence,
            mut attributes,
        } = candidate;
        if let Some(projection_id) = projection_id {
            attributes.insert("legacy_projection_id".to_owned(), projection_id.to_owned());
        }
        let entity = SoftwareEntity::new(SoftwareEntityInput {
            repository_id: self.repository_id.clone(),
            source_scope: self.source_scope.clone(),
            entity_kind: kind,
            name,
            namespace,
            source_kind,
            evidence_refs: vec![evidence],
            attributes,
            created_graph_version: self.graph_version,
        })
        .map_err(domain_error)?;
        let entity_key = entity.entity_key.clone();
        if self.occurrence_ids.insert(entity.occurrence_id.clone()) {
            if self.entities.len() >= MAX_ONTOLOGY_ENTITIES_PER_SCOPE {
                return Err(StorageError::CapacityExceeded(format!(
                    "software ontology entities exceed the bounded limit {MAX_ONTOLOGY_ENTITIES_PER_SCOPE}"
                )));
            }
            self.entities.push(entity);
        }
        Ok(entity_key)
    }

    fn add_file(
        &mut self,
        projection_id: &str,
        path: &str,
        language_id: &str,
        file_role: &str,
        parse_status: &str,
    ) -> Result<String, StorageError> {
        let evidence = self.evidence(path, 1, 1)?;
        let source_kind = source_kind_for(file_role, path);
        let mut attributes = BTreeMap::new();
        attributes.insert("language_id".to_owned(), language_id.to_owned());
        attributes.insert("file_role".to_owned(), file_role.to_owned());
        attributes.insert("parse_status".to_owned(), parse_status.to_owned());
        let key = self.add_entity(OntologyEntityCandidate {
            projection_id: Some(projection_id),
            kind: SoftwareEntityKind::FileRevision,
            name: path.to_owned(),
            namespace: None,
            source_kind,
            evidence: evidence.clone(),
            attributes,
        })?;
        self.files_by_path.insert(path.to_owned(), key.clone());
        self.add_statement(
            self.snapshot_key.clone(),
            SoftwarePredicate::Contains,
            Some(key.clone()),
            None,
            source_kind,
            evidence.clone(),
            SoftwareAssertionMode::Extracted,
            SoftwareStatementResolution::Resolved,
            10_000,
        )?;
        if source_kind == SoftwareSourceKind::ApiSchema {
            self.add_api_schema(projection_id, path, language_id, evidence)?;
        }
        Ok(key)
    }

    fn add_api_schema(
        &mut self,
        projection_id: &str,
        path: &str,
        language_id: &str,
        evidence: SoftwareEvidenceRef,
    ) -> Result<(), StorageError> {
        let mut attributes = BTreeMap::new();
        attributes.insert("language_id".to_owned(), language_id.to_owned());
        attributes.insert("schema_path".to_owned(), path.to_owned());
        let api_key = self.add_entity(OntologyEntityCandidate {
            projection_id: Some(projection_id),
            kind: SoftwareEntityKind::Api,
            name: path.to_owned(),
            namespace: Some(path.to_owned()),
            source_kind: SoftwareSourceKind::ApiSchema,
            evidence: evidence.clone(),
            attributes,
        })?;
        self.add_statement(
            self.snapshot_key.clone(),
            SoftwarePredicate::Contains,
            Some(api_key),
            None,
            SoftwareSourceKind::ApiSchema,
            evidence,
            SoftwareAssertionMode::Declared,
            SoftwareStatementResolution::Resolved,
            10_000,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn add_statement(
        &mut self,
        subject_id: String,
        predicate: SoftwarePredicate,
        object_id: Option<String>,
        object_value: Option<String>,
        source_kind: SoftwareSourceKind,
        evidence: SoftwareEvidenceRef,
        assertion_mode: SoftwareAssertionMode,
        resolution_state: SoftwareStatementResolution,
        confidence_basis_points: u16,
    ) -> Result<(), StorageError> {
        let statement = SoftwareStatement::candidate(SoftwareStatementInput {
            subject_id,
            predicate,
            object_id,
            object_value,
            source_scope: self.source_scope.clone(),
            source_kind,
            evidence_refs: vec![evidence],
            assertion_mode,
            resolution_state,
            valid_from: None,
            valid_to: None,
            observed_at: None,
            extractor_id: EXTRACTOR_ID.to_owned(),
            extractor_version: EXTRACTOR_VERSION.to_owned(),
            confidence_basis_points,
            fact_state: SoftwareFactState::Active,
        });
        if self.statement_ids.insert(statement.statement_id.clone()) {
            if self.statements.len() >= MAX_ONTOLOGY_STATEMENTS_PER_SCOPE {
                return Err(StorageError::CapacityExceeded(format!(
                    "software ontology statements exceed the bounded limit {MAX_ONTOLOGY_STATEMENTS_PER_SCOPE}"
                )));
            }
            self.statements.push(statement);
        }
        Ok(())
    }

    fn file_key(&self, path: &str) -> Option<String> {
        self.files_by_path.get(path).cloned()
    }
}

fn persist_entities(
    connection: &Connection,
    entities: &[SoftwareEntity],
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        INSERT INTO software_entities (
            occurrence_id, entity_key, repository_id, source_scope, entity_kind, name,
            namespace, source_kind, primary_evidence_path, language_id,
            evidence_refs_json, attributes_json, created_graph_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ",
    )?;
    for entity in entities {
        let evidence_json = serde_json::to_string(&entity.evidence_refs)
            .map_err(|error| serialization_error("entity evidence", error))?;
        let attributes_json = serde_json::to_string(&entity.attributes)
            .map_err(|error| serialization_error("entity attributes", error))?;
        let evidence_path = entity
            .evidence_refs
            .first()
            .map_or("", |evidence| evidence.path.as_str());
        let language_id = entity
            .attributes
            .get("language_id")
            .map_or("unknown", String::as_str);
        statement.execute(params![
            entity.occurrence_id,
            entity.entity_key,
            entity.repository_id,
            entity.source_scope,
            entity.entity_kind.as_str(),
            entity.name,
            entity.namespace,
            entity.source_kind.as_str(),
            evidence_path,
            language_id,
            evidence_json,
            attributes_json,
            entity.created_graph_version.get(),
        ])?;
    }
    Ok(())
}

fn persist_statements(
    connection: &Connection,
    statements: &[SoftwareStatement],
) -> Result<(), StorageError> {
    let mut insert = connection.prepare(
        "
        INSERT INTO software_statements (
            statement_id, source_scope, subject_id, predicate, object_id, object_value,
            source_kind, evidence_refs_json, primary_evidence_path, assertion_mode,
            resolution_state, valid_from, valid_to, observed_at, extractor_id,
            extractor_version, confidence_basis_points, fact_state
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                  ?15, ?16, ?17, ?18)
        ",
    )?;
    for statement in statements {
        let evidence_json = serde_json::to_string(&statement.evidence_refs)
            .map_err(|error| serialization_error("statement evidence", error))?;
        let evidence_path = statement
            .evidence_refs
            .first()
            .map_or("", |evidence| evidence.path.as_str());
        insert.execute(params![
            statement.statement_id,
            statement.source_scope,
            statement.subject_id,
            statement.predicate.as_str(),
            statement.object_id,
            statement.object_value,
            statement.source_kind.as_str(),
            evidence_json,
            evidence_path,
            statement.assertion_mode.as_str(),
            statement.resolution_state.as_str(),
            statement.valid_from,
            statement.valid_to,
            statement.observed_at,
            statement.extractor_id,
            statement.extractor_version,
            statement.confidence_basis_points,
            statement.fact_state.as_str(),
        ])?;
    }
    Ok(())
}

fn persist_diagnostics(
    connection: &Connection,
    source_scope: &str,
    diagnostics: &[SoftwareShapeDiagnostic],
) -> Result<(), StorageError> {
    let mut insert = connection.prepare(
        "
        INSERT INTO software_ontology_diagnostics (
            diagnostic_id, source_scope, shape_id, code, severity, statement_id,
            entity_key, field, message
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
    )?;
    for diagnostic in diagnostics {
        insert.execute(params![
            diagnostic.diagnostic_id,
            source_scope,
            diagnostic.shape_id,
            diagnostic.code,
            diagnostic.severity.as_str(),
            diagnostic.statement_id,
            diagnostic.entity_key,
            diagnostic.field,
            diagnostic.message,
        ])?;
    }
    Ok(())
}

fn source_coverage(statements: &[SoftwareStatement]) -> SoftwareSourceCoverage {
    let source_kinds = statements
        .iter()
        .map(|statement| statement.source_kind)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let source_paths = statements
        .iter()
        .flat_map(|statement| statement.evidence_refs.iter())
        .map(|evidence| evidence.path.as_str())
        .collect::<BTreeSet<_>>();
    let evidence_ref_count = statements
        .iter()
        .map(|statement| statement.evidence_refs.len())
        .sum();
    SoftwareSourceCoverage {
        source_kinds,
        source_path_count: source_paths.len(),
        evidence_ref_count,
    }
}

fn provenance_completeness(statements: &[SoftwareStatement]) -> u16 {
    let active = statements
        .iter()
        .filter(|statement| statement.fact_state == SoftwareFactState::Active)
        .collect::<Vec<_>>();
    if active.is_empty() {
        return 10_000;
    }
    let complete = active
        .iter()
        .filter(|statement| {
            !statement.source_scope.is_empty()
                && !statement.evidence_refs.is_empty()
                && !statement.extractor_id.is_empty()
                && !statement.extractor_version.is_empty()
        })
        .count();
    ((complete * 10_000) / active.len()) as u16
}

fn conflict_count(statements: &[SoftwareStatement]) -> usize {
    statements
        .iter()
        .filter(|statement| statement.fact_state == SoftwareFactState::Conflicting)
        .map(|statement| (statement.subject_id.as_str(), statement.predicate))
        .collect::<BTreeSet<_>>()
        .len()
}

fn source_kind_for(source_kind: &str, path: &str) -> SoftwareSourceKind {
    let source = source_kind.to_ascii_lowercase();
    let path = path.to_ascii_lowercase();
    if source == "api_schema" || source.contains("openapi") || source.contains("swagger") {
        SoftwareSourceKind::ApiSchema
    } else if source.contains("lock") || path.ends_with(".lock") || path.ends_with("lock.json") {
        SoftwareSourceKind::Lockfile
    } else if source.contains("github-actions")
        || source.contains("gitlab-ci")
        || path.starts_with(".github/workflows/")
    {
        SoftwareSourceKind::Ci
    } else if source.contains("dockerfile")
        || source.contains("cmake")
        || source.contains("makefile")
        || source.contains("gradle")
    {
        SoftwareSourceKind::BuildFile
    } else if source.contains("systemd") || source.contains("launchd") {
        SoftwareSourceKind::ServiceDefinition
    } else if source.contains("terraform")
        || source.contains("kubernetes")
        || source.contains("compose")
        || source.contains("helm")
        || source == "deployment"
    {
        SoftwareSourceKind::Iac
    } else if source.contains("markdown") || source == "documentation" {
        SoftwareSourceKind::Documentation
    } else if source == "test" || path.contains("/tests/") || path.starts_with("tests/") {
        SoftwareSourceKind::Test
    } else if source.contains("manifest")
        || matches!(
            source.as_str(),
            "cargo.toml" | "package.json" | "pyproject.toml" | "go.mod"
        )
    {
        SoftwareSourceKind::Manifest
    } else {
        SoftwareSourceKind::Code
    }
}

fn resolution_state(value: &str) -> SoftwareStatementResolution {
    match value {
        "unresolved" => SoftwareStatementResolution::Unresolved,
        "ambiguous" => SoftwareStatementResolution::Ambiguous,
        "external" => SoftwareStatementResolution::External,
        "conflicting" => SoftwareStatementResolution::Conflicting,
        _ => SoftwareStatementResolution::Resolved,
    }
}

fn domain_error(error: crate::domain::DomainError) -> StorageError {
    StorageError::InvalidInput(error.to_string())
}

fn serialization_error(label: &str, error: serde_json::Error) -> StorageError {
    StorageError::Invariant(format!(
        "software ontology {label} cannot be serialized: {error}"
    ))
}
