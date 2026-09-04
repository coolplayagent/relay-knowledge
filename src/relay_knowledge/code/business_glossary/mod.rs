//! Loads route-authorized business glossaries from immutable Git snapshots.

use std::{
    collections::HashSet,
    path::{Component, Path},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    domain::{
        BusinessGlossary, BusinessKnowledgeProjectionInput, BusinessKnowledgeSource,
        CodeRepositoryRegistration, KnowledgeMap, KnowledgeMapRoute, KnowledgeMapSource,
        KnowledgeMapSourceKind, KnowledgeMapTopic,
    },
    project::{
        BUSINESS_GLOSSARY_RELATIVE_PATH, KNOWLEDGE_MAP_RELATIVE_PATH,
        KNOWLEDGE_MAP_TOPICS_RELATIVE_PREFIX, LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH,
        LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH,
    },
};

use super::{
    CodeIndexError,
    source::{
        RepositorySourceKind, source_blob_sizes_after_policy_verification, source_kind,
        source_snapshot_bytes,
    },
};

const BUSINESS_TOPIC_ID: &str = "business-knowledge";
const KNOWLEDGE_MAP_V2_SCHEMA: u16 = 2;
const KNOWLEDGE_MAP_V3_SCHEMA: u16 = 3;
const KNOWLEDGE_MAP_V4_SCHEMA: u16 = 4;

#[derive(Deserialize)]
struct SchemaProbe {
    schema_version: u16,
}

#[derive(Deserialize)]
struct ShardedManifest {
    schema_version: u16,
    topics: Vec<ShardedTopicRef>,
}

#[derive(Deserialize)]
struct ShardedTopicRef {
    id: String,
    title: String,
    description: String,
    source_ids: Vec<String>,
    #[serde(rename = "ref")]
    shard_ref: String,
    digest: String,
}

#[derive(Deserialize)]
struct ShardedTopicShard {
    schema_version: u16,
    topic: KnowledgeMapTopic,
    sources: Vec<KnowledgeMapSource>,
    route: Option<KnowledgeMapRoute>,
}

struct RoutedBusinessSources {
    route: KnowledgeMapRoute,
    sources: Vec<KnowledgeMapSource>,
}

/// Reads only the business-knowledge route and its files from one immutable Git commit.
pub(crate) fn load_business_knowledge_projection(
    registration: &CodeRepositoryRegistration,
    source_scope: &str,
    resolved_commit_sha: &str,
) -> Result<BusinessKnowledgeProjectionInput, CodeIndexError> {
    let root = Path::new(&registration.root_path);
    let kind = source_kind(root)?;
    if kind == RepositorySourceKind::FileSystem {
        return Ok(empty_projection(
            registration,
            source_scope,
            resolved_commit_sha,
        ));
    }
    let map_path =
        if snapshot_blob_size(root, resolved_commit_sha, KNOWLEDGE_MAP_RELATIVE_PATH)?.is_some() {
            KNOWLEDGE_MAP_RELATIVE_PATH
        } else if snapshot_blob_size(
            root,
            resolved_commit_sha,
            LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH,
        )?
        .is_some()
        {
            LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH
        } else {
            return Ok(empty_projection(
                registration,
                source_scope,
                resolved_commit_sha,
            ));
        };
    let map_content = source_snapshot_bytes(root, kind, resolved_commit_sha, map_path)?;
    let routed = routed_business_sources(root, kind, resolved_commit_sha, map_path, &map_content)?;
    let Some(routed) = routed else {
        return Ok(empty_projection(
            registration,
            source_scope,
            resolved_commit_sha,
        ));
    };
    let mut sources = Vec::with_capacity(routed.route.source_order.len());
    for source_id in &routed.route.source_order {
        if source_id != "repository-business-glossary" {
            continue;
        }
        let source = routed
            .sources
            .iter()
            .find(|source| &source.id == source_id)
            .ok_or_else(|| invalid(format!("route references missing source '{source_id}'")))?;
        validate_routed_source(source, map_path == LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH)?;
        validate_repository_path(&source.uri)?;
        let size =
            snapshot_blob_size(root, resolved_commit_sha, &source.uri)?.ok_or_else(|| {
                invalid(format!(
                    "routed glossary source '{}' does not exist at commit {resolved_commit_sha}",
                    source.uri
                ))
            })?;
        if size > crate::domain::BUSINESS_GLOSSARY_MAX_BYTES {
            return Err(invalid(format!(
                "routed glossary source '{}' exceeds 4194304 bytes",
                source.uri
            )));
        }
        let content = source_snapshot_bytes(root, kind, resolved_commit_sha, &source.uri)?;
        let glossary = BusinessGlossary::parse(&content)
            .map_err(|error| invalid(format!("source '{}': {error}", source.uri)))?;
        sources.push(BusinessKnowledgeSource {
            source_id: source.id.clone(),
            source_path: source.uri.clone(),
            authority_rank: sources.len(),
            content_digest: sha256(&content),
            glossary,
        });
    }

    Ok(BusinessKnowledgeProjectionInput {
        repository_id: registration.repository_id.clone(),
        source_scope: source_scope.to_owned(),
        resolved_commit_sha: resolved_commit_sha.to_owned(),
        sources,
    })
}

fn empty_projection(
    registration: &CodeRepositoryRegistration,
    source_scope: &str,
    resolved_commit_sha: &str,
) -> BusinessKnowledgeProjectionInput {
    BusinessKnowledgeProjectionInput {
        repository_id: registration.repository_id.clone(),
        source_scope: source_scope.to_owned(),
        resolved_commit_sha: resolved_commit_sha.to_owned(),
        sources: Vec::new(),
    }
}

fn snapshot_blob_size(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<usize>, CodeIndexError> {
    source_blob_sizes_after_policy_verification(root, commit, &[path.to_owned()])?
        .into_iter()
        .next()
        .ok_or_else(|| CodeIndexError::Invariant("blob-size query returned no row".to_owned()))
}

fn routed_business_sources(
    root: &Path,
    kind: RepositorySourceKind,
    commit: &str,
    map_path: &str,
    content: &[u8],
) -> Result<Option<RoutedBusinessSources>, CodeIndexError> {
    let probe = serde_norway::from_slice::<SchemaProbe>(content)
        .map_err(|error| invalid(format!("knowledge map YAML is invalid: {error}")))?;
    if probe.schema_version == KnowledgeMap::SCHEMA_VERSION {
        let map = serde_norway::from_slice::<KnowledgeMap>(content)
            .map_err(|error| invalid(format!("knowledge map YAML is invalid: {error}")))?;
        let mut validation_map = map.clone();
        if map_path == LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH {
            normalize_legacy_glossary_uri(&mut validation_map);
        }
        validation_map
            .validate()
            .map_err(|error| invalid(format!("knowledge map is invalid: {error}")))?;
        return Ok(route_from_parts(map.routes, map.sources));
    }
    if !matches!(
        probe.schema_version,
        KNOWLEDGE_MAP_V2_SCHEMA | KNOWLEDGE_MAP_V3_SCHEMA | KNOWLEDGE_MAP_V4_SCHEMA
    ) {
        return Err(invalid(format!(
            "knowledge map schema_version {} is unsupported",
            probe.schema_version
        )));
    }
    let manifest = serde_norway::from_slice::<ShardedManifest>(content)
        .map_err(|error| invalid(format!("knowledge map manifest is invalid: {error}")))?;
    if manifest.schema_version != probe.schema_version {
        return Err(invalid("knowledge map manifest schema drift"));
    }
    let Some(reference) = manifest
        .topics
        .iter()
        .find(|reference| reference.id == BUSINESS_TOPIC_ID)
    else {
        return Ok(None);
    };
    let contract_dir = map_path
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    validate_sharded_ref(reference, contract_dir)?;
    let snapshot_path = format!("{contract_dir}/{}", reference.shard_ref);
    let shard_content = source_snapshot_bytes(root, kind, commit, &snapshot_path)?;
    if sha256(&shard_content) != reference.digest {
        return Err(invalid(format!(
            "knowledge map topic shard '{}' digest mismatch",
            reference.shard_ref
        )));
    }
    let shard = serde_norway::from_slice::<ShardedTopicShard>(&shard_content)
        .map_err(|error| invalid(format!("business topic shard is invalid: {error}")))?;
    if shard.schema_version != probe.schema_version
        || shard.topic.id != reference.id
        || shard.topic.title != reference.title
        || shard.topic.description != reference.description
        || !shard
            .sources
            .iter()
            .map(|source| source.id.as_str())
            .eq(reference.source_ids.iter().map(String::as_str))
    {
        return Err(invalid(
            "business topic shard identity does not match manifest",
        ));
    }
    validate_sharded_topic(&shard)?;
    let route = shard.route.ok_or_else(|| {
        invalid("business topic shard must route reserved source 'repository-business-glossary'")
    })?;
    if !route
        .source_order
        .iter()
        .any(|source_id| source_id == "repository-business-glossary")
    {
        return Err(invalid(
            "business topic route must include reserved source 'repository-business-glossary'",
        ));
    }
    Ok(Some(RoutedBusinessSources {
        route,
        sources: shard.sources,
    }))
}

fn normalize_legacy_glossary_uri(map: &mut KnowledgeMap) {
    for source in &mut map.sources {
        if source.id == "repository-business-glossary"
            && source.uri == LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH
        {
            source.uri = BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned();
        }
    }
}

fn route_from_parts(
    routes: Vec<KnowledgeMapRoute>,
    sources: Vec<KnowledgeMapSource>,
) -> Option<RoutedBusinessSources> {
    routes
        .into_iter()
        .find(|route| route.topic == BUSINESS_TOPIC_ID)
        .map(|route| RoutedBusinessSources { route, sources })
}

fn validate_sharded_topic(shard: &ShardedTopicShard) -> Result<(), CodeIndexError> {
    let mut source_ids = HashSet::with_capacity(shard.sources.len());
    for source in &shard.sources {
        if source.topic != shard.topic.id || !source_ids.insert(source.id.as_str()) {
            return Err(invalid(format!(
                "business topic shard '{}' contains a foreign or duplicate source",
                shard.topic.id
            )));
        }
    }
    if let Some(route) = &shard.route {
        let mut routed = HashSet::with_capacity(route.source_order.len());
        if route.topic != shard.topic.id
            || route
                .source_order
                .iter()
                .any(|id| !source_ids.contains(id.as_str()) || !routed.insert(id.as_str()))
            || routed.len() != source_ids.len()
        {
            return Err(invalid(format!(
                "business topic shard '{}' has an invalid route",
                shard.topic.id
            )));
        }
    } else if !shard.sources.is_empty() {
        return Err(invalid(format!(
            "business topic shard '{}' has sources without a route",
            shard.topic.id
        )));
    }
    Ok(())
}

fn validate_sharded_ref(
    reference: &ShardedTopicRef,
    contract_dir: &str,
) -> Result<(), CodeIndexError> {
    if !reference.shard_ref.starts_with("topics/")
        || (contract_dir == "knowledge"
            && !format!("{contract_dir}/{}", reference.shard_ref)
                .starts_with(KNOWLEDGE_MAP_TOPICS_RELATIVE_PREFIX))
        || reference.digest.len() != 64
        || !reference
            .digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("business topic shard ref or digest is invalid"));
    }
    validate_repository_path(&format!("{contract_dir}/{}", reference.shard_ref))
}

fn validate_routed_source(
    source: &KnowledgeMapSource,
    legacy_contract: bool,
) -> Result<(), CodeIndexError> {
    if source.topic != BUSINESS_TOPIC_ID
        || source.kind != KnowledgeMapSourceKind::File
        || source.source_scope.as_deref() != Some("repo")
        || source.status != "active"
    {
        return Err(invalid(format!(
            "business source '{}' must be an active repository-scoped file",
            source.id
        )));
    }
    let expected_uri = BUSINESS_GLOSSARY_RELATIVE_PATH;
    let accepts_legacy_uri =
        legacy_contract && source.uri == LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH;
    if source.uri != expected_uri && !accepts_legacy_uri {
        return Err(invalid(format!(
            "reserved source 'repository-business-glossary' must use uri '{expected_uri}'"
        )));
    }
    Ok(())
}

fn validate_repository_path(path: &str) -> Result<(), CodeIndexError> {
    let value = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || value.is_absolute()
        || value.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid(format!("unsafe repository source path '{path}'")));
    }
    Ok(())
}

fn sha256(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn invalid(message: impl Into<String>) -> CodeIndexError {
    CodeIndexError::InvalidInput(format!("business knowledge projection: {}", message.into()))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
