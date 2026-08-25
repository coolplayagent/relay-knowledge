//! Extracts topic identities from the repository knowledge-map contract.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path};

use super::{
    model::ConfigFact,
    source::{push_definition, source_lines, unquote},
};
use crate::domain::{
    KnowledgeMap, KnowledgeMapHistoryEntry, KnowledgeMapRoute, KnowledgeMapSource,
    KnowledgeMapTopic,
};
use crate::project::{
    AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_HISTORY_DIR_NAME, KNOWLEDGE_MAP_RELATIVE_PATH,
    KNOWLEDGE_MAP_TOPICS_DIR_NAME, KNOWLEDGE_MAP_TOPICS_RELATIVE_PREFIX,
};

const ARTIFACT_SCHEMA_VERSION: u16 = 2;
const RECENT_HISTORY_LIMIT: usize = 16;

#[derive(Deserialize)]
struct SchemaProbe {
    schema_version: u16,
}

#[derive(Deserialize)]
struct RootManifest {
    schema_version: u16,
    map_version: u64,
    #[serde(rename = "updated_at")]
    _updated_at: String,
    topics: Vec<RootTopicRef>,
    history: RootHistory,
}

#[derive(Deserialize)]
struct RootTopicRef {
    id: String,
    title: String,
    description: String,
    source_ids: Vec<String>,
    #[serde(rename = "ref")]
    shard_ref: String,
    digest: String,
}

#[derive(Deserialize)]
struct RootHistory {
    archived_through: u64,
    #[serde(default)]
    archive: Option<RootArchiveRef>,
    #[serde(default)]
    index: Option<RootHistoryIndexRef>,
    recent: Vec<RootHistoryEntry>,
}

#[derive(Deserialize)]
struct RootArchiveRef {
    #[serde(rename = "ref")]
    archive_ref: String,
    digest: String,
}

#[derive(Deserialize)]
struct RootHistoryIndexRef {
    from_version: u64,
    through_version: u64,
    height: u8,
    #[serde(rename = "ref")]
    index_ref: String,
    digest: String,
}

#[derive(Deserialize)]
struct RootHistoryEntry {
    version: u64,
    action: String,
    actor: String,
    summary: String,
}

#[derive(Deserialize)]
struct TopicShard {
    schema_version: u16,
    topic: KnowledgeMapTopic,
    sources: Vec<KnowledgeMapSource>,
    #[serde(default)]
    route: Option<KnowledgeMapRoute>,
}

#[derive(Deserialize)]
struct TopicIdentity {
    id: String,
}

#[cfg(test)]
mod mod_tests;

pub(super) fn facts(
    path: &str,
    language_id: &str,
    content: &str,
    definitions: &mut Vec<ConfigFact>,
) {
    if language_id != "yaml" {
        return;
    }

    if path == KNOWLEDGE_MAP_RELATIVE_PATH {
        record_root_facts(content, definitions);
    } else if topic_shard_path(path).is_some() {
        record_topic_shard_fact(path, content, definitions);
    }
}

fn record_root_facts(content: &str, definitions: &mut Vec<ConfigFact>) {
    let Ok(probe) = serde_norway::from_str::<SchemaProbe>(content) else {
        return;
    };
    if probe.schema_version == 1 {
        record_root_topic_ids(content, definitions);
        return;
    }
    if probe.schema_version != ARTIFACT_SCHEMA_VERSION {
        return;
    }
    let Ok(manifest) = serde_norway::from_str::<RootManifest>(content) else {
        return;
    };
    if !valid_manifest(&manifest) {
        return;
    }
    for topic in manifest.topics {
        if !valid_topic_ref(&topic) {
            continue;
        }
        let Some(range) = topic_ref_range(content, &topic) else {
            continue;
        };
        push_definition(
            definitions,
            format!("{AGENT_CONTRACT_DIR_NAME}/{}", topic.shard_ref),
            "knowledge_map_topic_shard_ref",
            range,
        );
        push_definition(
            definitions,
            &topic.id,
            "knowledge_map_topic_shard_topic",
            range,
        );
        push_definition(
            definitions,
            topic_identity_digest(
                &topic.id,
                &topic.title,
                &topic.description,
                topic.source_ids.iter().map(String::as_str),
            ),
            "knowledge_map_topic_shard_identity",
            range,
        );
    }
}

fn valid_manifest(manifest: &RootManifest) -> bool {
    if manifest.schema_version != ARTIFACT_SCHEMA_VERSION || manifest.map_version == 0 {
        return false;
    }
    let mut topic_ids = HashSet::new();
    let mut folded_topic_ids = HashSet::new();
    let mut refs = HashSet::new();
    let mut folded_refs = HashSet::new();
    let mut source_ids = HashSet::new();
    if manifest.topics.iter().any(|topic| {
        let mut topic_source_ids = HashSet::new();
        topic.id.trim().is_empty()
            || topic.title.trim().is_empty()
            || topic.description.trim().is_empty()
            || !valid_topic_ref(topic)
            || !topic_ids.insert(topic.id.as_str())
            || !folded_topic_ids.insert(topic.id.to_lowercase())
            || !refs.insert(topic.shard_ref.as_str())
            || !folded_refs.insert(topic.shard_ref.to_lowercase())
            || topic.source_ids.iter().any(|source_id| {
                source_id.trim().is_empty()
                    || !topic_source_ids.insert(source_id.as_str())
                    || !source_ids.insert(source_id.as_str())
            })
    }) {
        return false;
    }
    valid_recent_history(manifest)
        && manifest.history.archive.as_ref().is_none_or(|archive| {
            scoped_contract_ref(&archive.archive_ref, KNOWLEDGE_MAP_HISTORY_DIR_NAME)
                && lower_hex(&archive.digest, 64)
        })
        && manifest.history.index.as_ref().is_none_or(|index| {
            index.from_version == 1
                && index.through_version == manifest.history.archived_through
                && index.height <= 10
                && lower_hex(&index.digest, 64)
                && index.index_ref
                    == format!(
                        "{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/index-{:02}-{:020}-{:020}-{}.yaml",
                        index.height, index.from_version, index.through_version, index.digest
                    )
        })
}

fn valid_recent_history(manifest: &RootManifest) -> bool {
    if manifest.history.recent.is_empty() || manifest.history.recent.len() > RECENT_HISTORY_LIMIT {
        return false;
    }
    let Some(mut expected) = manifest.history.archived_through.checked_add(1) else {
        return false;
    };
    for entry in &manifest.history.recent {
        if entry.version != expected
            || entry.action.trim().is_empty()
            || entry.actor.trim().is_empty()
            || entry.summary.trim().is_empty()
        {
            return false;
        }
        let Some(next) = expected.checked_add(1) else {
            return false;
        };
        expected = next;
    }
    if expected.saturating_sub(1) != manifest.map_version
        || (manifest.history.archived_through == 0) != manifest.history.archive.is_none()
        || (manifest.history.archived_through == 0 && manifest.history.index.is_some())
    {
        return false;
    }
    manifest
        .history
        .archive
        .as_ref()
        .is_none_or(|archive| valid_archive_ref_shape(archive, manifest.history.archived_through))
}

fn valid_archive_ref_shape(archive: &RootArchiveRef, archived_through: u64) -> bool {
    let name = archive
        .archive_ref
        .strip_prefix(&format!("{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/"))
        .and_then(|value| value.strip_suffix(".yaml"));
    let Some(name) = name else {
        return false;
    };
    let mut parts = name.split('-');
    let from_text = parts.next();
    let through_text = parts.next();
    let from = from_text.and_then(|value| value.parse::<u64>().ok());
    let through = through_text.and_then(|value| value.parse::<u64>().ok());
    let digest = parts.next();
    parts.next().is_none()
        && from.is_some_and(|value| value > 0 && value <= archived_through)
        && from_text.is_some_and(|value| value.len() == 20)
        && through_text.is_some_and(|value| value.len() == 20)
        && through == Some(archived_through)
        && digest == Some(archive.digest.as_str())
}

fn scoped_contract_ref(relative: &str, directory: &str) -> bool {
    let path = Path::new(relative);
    let mut components = path.components();
    !path.is_absolute()
        && matches!(components.next(), Some(std::path::Component::Normal(value)) if value == directory)
        && matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn topic_ref_range(content: &str, topic: &RootTopicRef) -> Option<super::model::ConfigRange> {
    let lines = source_lines(content);
    lines
        .iter()
        .find_map(|line| {
            let value = yaml_scalar(line.text, "ref")?;
            (value == topic.shard_ref).then(|| line.range())
        })
        .or_else(|| {
            lines.into_iter().find_map(|line| {
                (line.text.contains(&topic.shard_ref)
                    && line.text.contains(&topic.digest)
                    && line.text.contains(&topic.id))
                .then(|| line.range())
            })
        })
}

fn record_root_topic_ids(content: &str, definitions: &mut Vec<ConfigFact>) {
    let mut in_topics = false;
    let mut topic_list_indent = None;
    let mut topic_item_indent = None;
    for line in source_lines(content) {
        let code = yaml_code_prefix(line.text);
        let trimmed = code.trim();
        if let Some(section) = top_level_yaml_section(code) {
            in_topics = section == "topics";
            topic_list_indent = None;
            topic_item_indent = None;
            continue;
        }
        if !in_topics || trimmed.is_empty() {
            continue;
        }

        let indent = leading_spaces(code);
        if let Some(item) = trimmed.strip_prefix("- ") {
            if !accept_topic_item_indent(&mut topic_list_indent, indent) {
                continue;
            }
            topic_item_indent = Some(indent);
            let item = item.trim_start();
            if let Some(id) = item.strip_prefix("id:") {
                push_topic_definition(definitions, id, line.range());
            }
            continue;
        }
        if trimmed == "-" {
            if !accept_topic_item_indent(&mut topic_list_indent, indent) {
                continue;
            }
            topic_item_indent = Some(indent);
            continue;
        }
        if topic_item_indent.is_some_and(|item_indent| indent == item_indent + 2) {
            let Some(id) = trimmed.strip_prefix("id:") else {
                continue;
            };
            push_topic_definition(definitions, id, line.range());
        }
    }
}

fn record_topic_shard_fact(path: &str, content: &str, definitions: &mut Vec<ConfigFact>) {
    let Some((path_topic_id, path_digest)) = topic_shard_path(path) else {
        return;
    };
    if content_digest(content.as_bytes()) != path_digest {
        return;
    }
    let Ok(shard) = serde_norway::from_str::<TopicShard>(content) else {
        return;
    };
    if shard.schema_version != ARTIFACT_SCHEMA_VERSION
        || stable_id(&shard.topic.id) != path_topic_id
        || !valid_topic_shard(&shard)
    {
        return;
    }
    let Some(range) = topic_id_range(content, &shard.topic.id) else {
        return;
    };
    push_definition(
        definitions,
        &shard.topic.id,
        "knowledge_map_topic_shard",
        range,
    );
    push_definition(
        definitions,
        topic_identity_digest(
            &shard.topic.id,
            &shard.topic.title,
            &shard.topic.description,
            shard.sources.iter().map(|source| source.id.as_str()),
        ),
        "knowledge_map_topic_shard_identity",
        range,
    );
}

fn valid_topic_shard(shard: &TopicShard) -> bool {
    KnowledgeMap {
        schema_version: KnowledgeMap::SCHEMA_VERSION,
        map_version: 1,
        updated_at: "shard-validation".to_owned(),
        topics: vec![shard.topic.clone()],
        sources: shard.sources.clone(),
        routes: shard.route.clone().into_iter().collect(),
        history: vec![KnowledgeMapHistoryEntry {
            version: 1,
            action: "validate".to_owned(),
            actor: "system".to_owned(),
            summary: "Validate an isolated topic shard.".to_owned(),
        }],
    }
    .validate()
    .is_ok()
}

fn topic_identity_digest<'a>(
    id: &'a str,
    title: &'a str,
    description: &'a str,
    source_ids: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut digest = Sha256::new();
    for value in [id, title, description].into_iter().chain(source_ids) {
        let length = u64::try_from(value.len()).unwrap_or(u64::MAX);
        digest.update(length.to_be_bytes());
        digest.update(value.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn topic_id_range(content: &str, expected_id: &str) -> Option<super::model::ConfigRange> {
    let mut in_topic = false;
    let lines = source_lines(content);
    for line in &lines {
        let code = yaml_code_prefix(line.text);
        let trimmed = code.trim();
        if !code.starts_with([' ', '\t'])
            && let Some(mapping) = trimmed.strip_prefix("topic:").map(str::trim)
            && flow_topic_id_matches(mapping, expected_id)
        {
            return Some(line.range());
        }
        if let Some(section) = top_level_yaml_section(code) {
            in_topic = section == "topic";
            continue;
        }
        if in_topic && leading_spaces(code) > 0 && yaml_scalar(code, "id") == Some(expected_id) {
            return Some(line.range());
        }
        if in_topic && leading_spaces(code) > 0 && flow_topic_id_matches(trimmed, expected_id) {
            return Some(line.range());
        }
    }
    lines.into_iter().find_map(|line| {
        (line.text.contains("topic") && line.text.contains(expected_id)).then(|| line.range())
    })
}

fn flow_topic_id_matches(content: &str, expected_id: &str) -> bool {
    if !content.starts_with('{') {
        return false;
    }
    serde_norway::from_str::<TopicIdentity>(content).is_ok_and(|topic| topic.id == expected_id)
}

fn valid_topic_ref(topic: &RootTopicRef) -> bool {
    !topic.id.trim().is_empty()
        && lower_hex(&topic.digest, 64)
        && topic.shard_ref
            == format!(
                "{}/topic-{}-{}.yaml",
                KNOWLEDGE_MAP_TOPICS_DIR_NAME,
                stable_id(&topic.id),
                topic.digest
            )
}

fn topic_shard_path(path: &str) -> Option<(String, String)> {
    let name = path
        .strip_prefix(KNOWLEDGE_MAP_TOPICS_RELATIVE_PREFIX)?
        .strip_prefix("topic-")?
        .strip_suffix(".yaml")?;
    if name.contains('/') || name.contains('\\') || name.len() != 16 + 1 + 64 {
        return None;
    }
    let (topic_id, digest) = name.split_once('-')?;
    (lower_hex(topic_id, 16) && lower_hex(digest, 64))
        .then(|| (topic_id.to_owned(), digest.to_owned()))
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn stable_id(value: &str) -> String {
    content_digest(value.as_bytes())[..16].to_owned()
}

fn content_digest(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn yaml_scalar<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let code = yaml_code_prefix(line).trim();
    let value = code
        .strip_prefix(key)
        .and_then(|value| value.strip_prefix(':'))
        .or_else(|| {
            code.strip_prefix('"')
                .and_then(|value| value.strip_prefix(key))
                .and_then(|value| value.strip_prefix("\":"))
        })
        .or_else(|| {
            code.strip_prefix('\'')
                .and_then(|value| value.strip_prefix(key))
                .and_then(|value| value.strip_prefix("':"))
        })?
        .trim();
    (!value.is_empty()).then(|| unquote(value))
}

fn push_topic_definition(
    definitions: &mut Vec<ConfigFact>,
    value: &str,
    range: super::model::ConfigRange,
) {
    let name = value.trim().trim_matches('"').trim_matches('\'');
    push_definition(definitions, name, "knowledge_map_topic", range);
}

fn accept_topic_item_indent(topic_list_indent: &mut Option<usize>, indent: usize) -> bool {
    match *topic_list_indent {
        Some(list_indent) => indent == list_indent,
        None => {
            *topic_list_indent = Some(indent);
            true
        }
    }
}

fn leading_spaces(line: &str) -> usize {
    line.chars()
        .take_while(|character| *character == ' ')
        .count()
}

fn yaml_code_prefix(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match character {
            '\\' if in_double && !escaped => escaped = true,
            '"' if !in_single && !escaped => in_double = !in_double,
            '\'' if !in_double => in_single = !in_single,
            '#' if !in_single && !in_double => return &line[..index],
            _ => escaped = false,
        }
        if character != '\\' {
            escaped = false;
        }
    }

    line
}

fn top_level_yaml_section(line: &str) -> Option<&str> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let key = line.trim().strip_suffix(':')?;
    if key.is_empty() || key.contains(' ') {
        return None;
    }

    Some(key)
}
