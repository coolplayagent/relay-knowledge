use std::{
    collections::{BTreeMap, BTreeSet},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use crate::{
    api::SoftwareGlobalResponse,
    domain::{
        SoftwareEntity, SoftwareEntityKind, SoftwareExportProfile, SoftwarePredicate,
        SoftwareStatement,
    },
};

const SPDX_CONTEXT: &str = "https://spdx.org/rdf/3.0.1/spdx-context.jsonld";
const CYCLONEDX_SCHEMA: &str = "http://cyclonedx.org/schema/bom-1.7.schema.json";
const PROV_NAMESPACE: &str = "http://www.w3.org/ns/prov#";
const ONTOLOGY_NAMESPACE: &str = "https://relay-knowledge.dev/ontology/software/1#";
const EXPORT_AGENT: &str = "urn:relay-knowledge:agent:relay-knowledge";

pub(super) fn export_document(
    response: &SoftwareGlobalResponse,
    profile: SoftwareExportProfile,
) -> Value {
    let created = generated_at();
    match profile {
        SoftwareExportProfile::Spdx3 => spdx_document(response, &created),
        SoftwareExportProfile::Cyclonedx17 => cyclonedx_document(response, &created),
        SoftwareExportProfile::ProvO => prov_document(response),
    }
}

fn spdx_document(response: &SoftwareGlobalResponse, created: &str) -> Value {
    let entities = unique_entities(&response.entities);
    let mut mapped_ids = BTreeMap::new();
    let mut elements = Vec::new();
    for entity in entities.values() {
        let Some(spdx_type) = spdx_type(entity.entity_kind) else {
            continue;
        };
        let id = export_id("spdx", &entity.entity_key);
        mapped_ids.insert(entity.entity_key.as_str(), id.clone());
        let mut element = json!({
            "spdxId": id,
            "type": spdx_type,
            "creationInfo": spdx_creation_info(created),
            "name": entity.name,
            "comment": format!(
                "relay-knowledge entity_kind={} source_scope={}",
                entity.entity_kind.as_str(), entity.source_scope
            )
        });
        if spdx_type == "software_Package" {
            if let Some(version) = entity.attributes.get("resolved_version") {
                element["software_packageVersion"] = json!(version);
            }
            element["software_primaryPurpose"] = json!(spdx_purpose(entity));
        }
        elements.push(element);
    }
    for statement in &response.statements {
        let (Some(subject), Some(object)) = (
            mapped_ids.get(statement.subject_id.as_str()),
            statement
                .object_id
                .as_deref()
                .and_then(|object| mapped_ids.get(object)),
        ) else {
            continue;
        };
        let Some(relationship_type) = spdx_relationship(statement.predicate) else {
            continue;
        };
        elements.push(json!({
            "spdxId": export_id("spdx-relationship", &statement.statement_id),
            "type": "Relationship",
            "creationInfo": spdx_creation_info(created),
            "from": subject,
            "relationshipType": relationship_type,
            "to": [object],
            "comment": format!(
                "relay-knowledge assertion_mode={} fact_state={}",
                statement.assertion_mode.as_str(), statement.fact_state.as_str()
            )
        }));
    }

    json!({
        "@context": SPDX_CONTEXT,
        "spdxId": export_id("spdx-document", &response.status.source_scope),
        "type": "SpdxDocument",
        "creationInfo": spdx_creation_info(created),
        "name": format!("{} software ontology", response.status.repository_id),
        "profileConformance": ["core", "software"],
        "element": elements
    })
}

fn spdx_creation_info(created: &str) -> Value {
    json!({
        "type": "CreationInfo",
        "created": created,
        "createdBy": [EXPORT_AGENT],
        "specVersion": "3.0.1"
    })
}

fn spdx_type(kind: SoftwareEntityKind) -> Option<&'static str> {
    match kind {
        SoftwareEntityKind::FileRevision => Some("software_File"),
        SoftwareEntityKind::SoftwareSystem
        | SoftwareEntityKind::Component
        | SoftwareEntityKind::BuildDefinition
        | SoftwareEntityKind::ReleaseArtifact
        | SoftwareEntityKind::PackageComponent
        | SoftwareEntityKind::Sdk
        | SoftwareEntityKind::RepositorySnapshot => Some("software_Package"),
        _ => None,
    }
}

fn spdx_purpose(entity: &SoftwareEntity) -> &'static str {
    match entity.entity_kind {
        SoftwareEntityKind::SoftwareSystem => "application",
        SoftwareEntityKind::BuildDefinition => "manifest",
        SoftwareEntityKind::ReleaseArtifact if entity.namespace.as_deref() == Some("container") => {
            "container"
        }
        SoftwareEntityKind::RepositorySnapshot => "source",
        SoftwareEntityKind::Sdk => "library",
        _ => "library",
    }
}

fn spdx_relationship(predicate: SoftwarePredicate) -> Option<&'static str> {
    match predicate {
        SoftwarePredicate::Contains => Some("contains"),
        SoftwarePredicate::DependsOn | SoftwarePredicate::ConsumesApi => Some("dependsOn"),
        SoftwarePredicate::Configures => Some("configures"),
        SoftwarePredicate::Builds | SoftwarePredicate::Produces => Some("generates"),
        SoftwarePredicate::Tests => Some("hasTest"),
        SoftwarePredicate::Documents => Some("hasDocumentation"),
        _ => None,
    }
}

fn cyclonedx_document(response: &SoftwareGlobalResponse, created: &str) -> Value {
    let entities = unique_entities(&response.entities);
    let mut components = Vec::new();
    let mut services = Vec::new();
    let mut mapped_ids = BTreeSet::new();
    for entity in entities.values() {
        let reference = export_id("entity", &entity.entity_key);
        if is_cyclonedx_service(entity.entity_kind) {
            services.push(json!({
                "bom-ref": reference,
                "name": entity.name,
                "properties": entity_properties(entity)
            }));
            mapped_ids.insert(entity.entity_key.as_str());
        } else if let Some(component_type) = cyclonedx_component_type(entity.entity_kind) {
            let mut component = json!({
                "bom-ref": reference,
                "type": component_type,
                "name": entity.name,
                "properties": entity_properties(entity)
            });
            if let Some(version) = entity.attributes.get("resolved_version") {
                component["version"] = json!(version);
            }
            components.push(component);
            mapped_ids.insert(entity.entity_key.as_str());
        }
    }
    let dependencies = cyclonedx_dependencies(&response.statements, &mapped_ids);

    json!({
        "$schema": CYCLONEDX_SCHEMA,
        "bomFormat": "CycloneDX",
        "specVersion": "1.7",
        "version": 1,
        "metadata": {
            "timestamp": created,
            "properties": [
                {"name": "relay-knowledge:ontology-version", "value": response.status.ontology_version},
                {"name": "relay-knowledge:source-scope", "value": response.status.source_scope}
            ]
        },
        "components": components,
        "services": services,
        "dependencies": dependencies,
        "properties": [
            {"name": "relay-knowledge:projection-schema-version", "value": response.status.projection_schema_version.to_string()},
            {"name": "relay-knowledge:provenance-completeness-basis-points", "value": response.status.completeness_basis_points.to_string()},
            {"name": "relay-knowledge:conflict-count", "value": response.status.conflict_count.to_string()}
        ]
    })
}

fn entity_properties(entity: &SoftwareEntity) -> Vec<Value> {
    let mut properties = vec![
        json!({"name": "relay-knowledge:entity-kind", "value": entity.entity_kind.as_str()}),
        json!({"name": "relay-knowledge:source-kind", "value": entity.source_kind.as_str()}),
        json!({"name": "relay-knowledge:occurrence-id", "value": entity.occurrence_id}),
    ];
    properties.extend(entity.attributes.iter().map(|(name, value)| {
        json!({
            "name": format!("relay-knowledge:extension:{name}"),
            "value": value
        })
    }));
    properties
}

fn is_cyclonedx_service(kind: SoftwareEntityKind) -> bool {
    matches!(
        kind,
        SoftwareEntityKind::Api | SoftwareEntityKind::RuntimeService
    )
}

fn cyclonedx_component_type(kind: SoftwareEntityKind) -> Option<&'static str> {
    match kind {
        SoftwareEntityKind::SoftwareSystem => Some("application"),
        SoftwareEntityKind::Component
        | SoftwareEntityKind::PackageComponent
        | SoftwareEntityKind::Sdk => Some("library"),
        SoftwareEntityKind::Configuration | SoftwareEntityKind::BuildDefinition => {
            Some("configuration")
        }
        SoftwareEntityKind::ReleaseArtifact => Some("container"),
        SoftwareEntityKind::FileRevision | SoftwareEntityKind::DocumentationUnit => Some("file"),
        SoftwareEntityKind::DeploymentUnit | SoftwareEntityKind::Resource => Some("platform"),
        SoftwareEntityKind::TestCase => Some("application"),
        _ => None,
    }
}

fn cyclonedx_dependencies<'a>(
    statements: &'a [SoftwareStatement],
    mapped_ids: &BTreeSet<&'a str>,
) -> Vec<Value> {
    let mut dependencies = BTreeMap::<&str, BTreeSet<&str>>::new();
    for statement in statements {
        if !matches!(
            statement.predicate,
            SoftwarePredicate::DependsOn
                | SoftwarePredicate::ConsumesApi
                | SoftwarePredicate::Deploys
        ) {
            continue;
        }
        let Some(object) = statement.object_id.as_deref() else {
            continue;
        };
        if mapped_ids.contains(statement.subject_id.as_str()) && mapped_ids.contains(object) {
            dependencies
                .entry(statement.subject_id.as_str())
                .or_default()
                .insert(object);
        }
    }
    dependencies
        .into_iter()
        .map(|(subject, objects)| {
            json!({
                "ref": export_id("entity", subject),
                "dependsOn": objects
                    .into_iter()
                    .map(|object| export_id("entity", object))
                    .collect::<Vec<_>>()
            })
        })
        .collect()
}

fn prov_document(response: &SoftwareGlobalResponse) -> Value {
    let entities = unique_entities(&response.entities);
    let mut graph = Vec::new();
    graph.push(json!({
        "@id": EXPORT_AGENT,
        "@type": "prov:SoftwareAgent",
        "prov:label": "relay-knowledge"
    }));
    let mut evidence = BTreeSet::new();
    for entity in entities.values() {
        let node_type = if entity.entity_kind == SoftwareEntityKind::BuildRun {
            "prov:Activity"
        } else {
            "prov:Entity"
        };
        graph.push(json!({
            "@id": export_id("entity", &entity.entity_key),
            "@type": node_type,
            "prov:label": entity.name,
            "rko:entityKind": entity.entity_kind.as_str(),
            "rko:sourceKind": entity.source_kind.as_str(),
            "rko:sourceScope": entity.source_scope,
            "rko:occurrenceId": entity.occurrence_id
        }));
        for reference in &entity.evidence_refs {
            push_prov_evidence(&mut graph, &mut evidence, reference);
        }
    }
    for statement in &response.statements {
        let statement_id = export_id("statement", &statement.statement_id);
        let activity_id = export_id("extraction", &statement.statement_id);
        let evidence_ids = statement
            .evidence_refs
            .iter()
            .map(|reference| {
                push_prov_evidence(&mut graph, &mut evidence, reference);
                export_id("evidence", &reference.evidence_id)
            })
            .collect::<Vec<_>>();
        let object = statement
            .object_id
            .as_deref()
            .map(|id| export_id("entity", id))
            .or_else(|| statement.object_value.clone());
        graph.push(json!({
            "@id": activity_id,
            "@type": "prov:Activity",
            "prov:used": evidence_ids,
            "prov:wasAssociatedWith": EXPORT_AGENT,
            "rko:extractorId": statement.extractor_id,
            "rko:extractorVersion": statement.extractor_version
        }));
        graph.push(json!({
            "@id": statement_id,
            "@type": ["prov:Entity", "rko:SoftwareStatement"],
            "prov:wasGeneratedBy": activity_id,
            "prov:wasDerivedFrom": evidence_ids,
            "rko:subject": export_id("entity", &statement.subject_id),
            "rko:predicate": statement.predicate.as_str(),
            "rko:object": object,
            "rko:assertionMode": statement.assertion_mode.as_str(),
            "rko:resolutionState": statement.resolution_state.as_str(),
            "rko:factState": statement.fact_state.as_str(),
            "rko:confidenceBasisPoints": statement.confidence_basis_points
        }));
    }

    json!({
        "@context": {
            "prov": PROV_NAMESPACE,
            "rko": ONTOLOGY_NAMESPACE
        },
        "@graph": graph
    })
}

fn push_prov_evidence(
    graph: &mut Vec<Value>,
    evidence_ids: &mut BTreeSet<String>,
    reference: &crate::domain::SoftwareEvidenceRef,
) {
    if !evidence_ids.insert(reference.evidence_id.clone()) {
        return;
    }
    graph.push(json!({
        "@id": export_id("evidence", &reference.evidence_id),
        "@type": "prov:Entity",
        "prov:atLocation": reference.path,
        "rko:sourceScope": reference.source_scope,
        "rko:lineStart": reference.line_range.start,
        "rko:lineEnd": reference.line_range.end
    }));
}

fn unique_entities(entities: &[SoftwareEntity]) -> BTreeMap<&str, &SoftwareEntity> {
    let mut unique = BTreeMap::new();
    for entity in entities {
        unique.entry(entity.entity_key.as_str()).or_insert(entity);
    }
    unique
}

fn export_id(kind: &str, id: &str) -> String {
    format!("urn:relay-knowledge:{kind}:{id}")
}

fn generated_at() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format_utc_timestamp(seconds)
}

fn format_utc_timestamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_date(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
