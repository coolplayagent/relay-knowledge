use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{DomainError, SourceScope, error::required_text};

/// Versioned repository contract that tells agents where project knowledge lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeMap {
    pub schema_version: u16,
    pub map_version: u64,
    pub updated_at: String,
    #[serde(default)]
    pub topics: Vec<KnowledgeMapTopic>,
    #[serde(default)]
    pub sources: Vec<KnowledgeMapSource>,
    #[serde(default)]
    pub routes: Vec<KnowledgeMapRoute>,
    #[serde(default)]
    pub history: Vec<KnowledgeMapHistoryEntry>,
}

impl KnowledgeMap {
    pub const SCHEMA_VERSION: u16 = 1;

    /// Creates the smallest valid shared contract.
    pub fn initial(updated_at: String) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            map_version: 1,
            updated_at,
            topics: Vec::new(),
            sources: Vec::new(),
            routes: Vec::new(),
            history: vec![KnowledgeMapHistoryEntry {
                version: 1,
                action: "init".to_owned(),
                actor: "cli".to_owned(),
                summary: "Created knowledge map.".to_owned(),
            }],
        }
    }

    /// Validates the cross-reference invariants that keep the map navigable.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(DomainError::invalid(
                "schema_version",
                format!("must be {}", Self::SCHEMA_VERSION),
            ));
        }
        if self.map_version == 0 {
            return Err(DomainError::invalid(
                "map_version",
                "must be greater than zero",
            ));
        }

        let mut topic_ids = HashSet::new();
        for topic in &self.topics {
            topic.validate()?;
            if !topic_ids.insert(topic.id.as_str()) {
                return Err(DomainError::invalid("topics", "topic ids must be unique"));
            }
        }

        let mut source_ids = HashSet::new();
        for source in &self.sources {
            source.validate()?;
            if !topic_ids.contains(source.topic.as_str()) {
                return Err(DomainError::invalid(
                    "sources",
                    format!("source '{}' references unknown topic", source.id),
                ));
            }
            if !source_ids.insert(source.id.as_str()) {
                return Err(DomainError::invalid("sources", "source ids must be unique"));
            }
        }

        let mut route_topics = HashSet::new();
        let mut routed_sources = HashSet::new();
        for route in &self.routes {
            route.validate()?;
            let mut route_sources = HashSet::new();
            if !route_topics.insert(route.topic.as_str()) {
                return Err(DomainError::invalid(
                    "routes",
                    "route topics must be unique",
                ));
            }
            if !topic_ids.contains(route.topic.as_str()) {
                return Err(DomainError::invalid(
                    "routes",
                    format!("route '{}' references unknown topic", route.topic),
                ));
            }
            for source_id in &route.source_order {
                if !route_sources.insert(source_id.as_str()) {
                    return Err(DomainError::invalid(
                        "routes",
                        format!("route '{}' repeats source '{}'", route.topic, source_id),
                    ));
                }
                let Some(source) = self.sources.iter().find(|source| source.id == *source_id)
                else {
                    return Err(DomainError::invalid(
                        "routes",
                        format!(
                            "route '{}' references unknown source '{}'",
                            route.topic, source_id
                        ),
                    ));
                };
                if source.topic != route.topic {
                    return Err(DomainError::invalid(
                        "routes",
                        format!(
                            "route '{}' references source '{}' from topic '{}'",
                            route.topic, source_id, source.topic
                        ),
                    ));
                }
                if !routed_sources.insert(source_id.as_str()) {
                    return Err(DomainError::invalid(
                        "routes",
                        format!("source '{}' appears in more than one route", source_id),
                    ));
                }
            }
        }
        for source in &self.sources {
            if !routed_sources.contains(source.id.as_str()) {
                return Err(DomainError::invalid(
                    "routes",
                    format!("source '{}' is not routed", source.id),
                ));
            }
        }

        self.validate_history()?;

        Ok(())
    }

    fn validate_history(&self) -> Result<(), DomainError> {
        if self.history.is_empty() {
            return Err(DomainError::invalid("history", "must not be empty"));
        }
        for (index, entry) in self.history.iter().enumerate() {
            entry.validate()?;
            let expected_version = u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| DomainError::invalid("history", "too many entries"))?;
            if entry.version != expected_version {
                return Err(DomainError::invalid(
                    "history",
                    "history versions must start at 1 and be contiguous",
                ));
            }
        }
        let latest_version = self
            .history
            .last()
            .map(|entry| entry.version)
            .expect("history is checked as non-empty");
        if latest_version != self.map_version {
            return Err(DomainError::invalid(
                "history",
                format!(
                    "latest history version {latest_version} must match map_version {}",
                    self.map_version
                ),
            ));
        }
        Ok(())
    }

    /// Adds a source to the map and creates a simple route for its topic when missing.
    pub fn add_source(&mut self, source: KnowledgeMapSource) -> Result<(), DomainError> {
        source.validate()?;
        if self.sources.iter().any(|entry| entry.id == source.id) {
            return Err(DomainError::invalid("id", "source already exists"));
        }
        if !self.topics.iter().any(|topic| topic.id == source.topic) {
            self.topics.push(KnowledgeMapTopic::new(
                source.topic.clone(),
                source.topic.clone(),
                "Added by CLI source registration.".to_owned(),
            )?);
        }
        let source_id = source.id.clone();
        let topic_id = source.topic.clone();
        self.sources.push(source);
        self.ensure_route_contains(&topic_id, &source_id)?;
        self.sort_entries();
        self.validate()
    }

    /// Applies supported source field updates without changing its identity.
    pub fn update_source(&mut self, change: KnowledgeMapChange) -> Result<(), DomainError> {
        let Some(source) = self.sources.iter_mut().find(|entry| entry.id == change.id) else {
            return Err(DomainError::invalid("id", "source does not exist"));
        };
        let previous_topic = source.topic.clone();
        if let Some(topic) = change.topic {
            source.topic = required_text("topic", topic)?;
        }
        if let Some(kind) = change.kind {
            source.kind = kind;
        }
        if let Some(uri) = change.uri {
            source.uri = required_text("uri", uri)?;
        }
        if let Some(scope) = change.source_scope {
            SourceScope::parse(scope.as_str())?;
            source.source_scope = Some(scope);
        }
        if let Some(description) = change.description {
            source.description = Some(required_text("description", description)?);
        }
        source.version = source.version.saturating_add(1);

        if !self.topics.iter().any(|topic| topic.id == source.topic) {
            self.topics.push(KnowledgeMapTopic::new(
                source.topic.clone(),
                source.topic.clone(),
                "Added by CLI source update.".to_owned(),
            )?);
        }
        let topic_id = source.topic.clone();
        let source_id = source.id.clone();
        if previous_topic != topic_id {
            self.prune_source_from_other_routes(&source_id, &topic_id);
        }
        self.ensure_route_contains(&topic_id, &source_id)?;
        self.sort_entries();
        self.validate()
    }

    /// Removes a source and prunes routes that referenced it.
    pub fn remove_source(&mut self, id: &str) -> Result<(), DomainError> {
        let before = self.sources.len();
        self.sources.retain(|source| source.id != id);
        if self.sources.len() == before {
            return Err(DomainError::invalid("id", "source does not exist"));
        }
        for route in &mut self.routes {
            route.source_order.retain(|source_id| source_id != id);
        }
        self.sort_entries();
        self.validate()
    }

    /// Advances the map version and records the mutation in history.
    pub fn record_change(&mut self, action: &str, summary: String, updated_at: String) {
        self.map_version = self.map_version.saturating_add(1);
        self.updated_at = updated_at;
        self.history.push(KnowledgeMapHistoryEntry {
            version: self.map_version,
            action: action.to_owned(),
            actor: "cli".to_owned(),
            summary,
        });
    }

    fn ensure_route_contains(&mut self, topic: &str, source_id: &str) -> Result<(), DomainError> {
        if let Some(route) = self.routes.iter_mut().find(|route| route.topic == topic) {
            if !route.source_order.iter().any(|id| id == source_id) {
                route.source_order.push(source_id.to_owned());
            }
            return Ok(());
        }
        self.routes.push(KnowledgeMapRoute {
            topic: topic.to_owned(),
            source_order: vec![source_id.to_owned()],
            fallback: Some("bounded-search".to_owned()),
        });
        Ok(())
    }

    fn prune_source_from_other_routes(&mut self, source_id: &str, current_topic: &str) {
        for route in &mut self.routes {
            if route.topic != current_topic {
                route.source_order.retain(|id| id != source_id);
            }
        }
    }

    fn sort_entries(&mut self) {
        self.topics.sort_by(|left, right| left.id.cmp(&right.id));
        self.sources.sort_by(|left, right| left.id.cmp(&right.id));
        self.routes
            .sort_by(|left, right| left.topic.cmp(&right.topic));
    }
}

/// Human-readable topic bucket used by agents for routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeMapTopic {
    pub id: String,
    pub title: String,
    pub description: String,
}

impl KnowledgeMapTopic {
    pub fn new(id: String, title: String, description: String) -> Result<Self, DomainError> {
        let topic = Self {
            id: required_text("topic", id)?,
            title: required_text("title", title)?,
            description: required_text("description", description)?,
        };
        topic.validate()?;
        Ok(topic)
    }

    fn validate(&self) -> Result<(), DomainError> {
        required_text("topic", self.id.as_str())?;
        required_text("title", self.title.as_str())?;
        required_text("description", self.description.as_str())?;
        Ok(())
    }
}

/// Addressable knowledge source that remains authoritative outside the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeMapSource {
    pub id: String,
    pub topic: String,
    pub kind: KnowledgeMapSourceKind,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub read_policy: String,
    pub write_policy: String,
    pub status: String,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl KnowledgeMapSource {
    pub fn new(
        id: String,
        topic: String,
        kind: KnowledgeMapSourceKind,
        uri: String,
        source_scope: Option<String>,
        description: Option<String>,
    ) -> Result<Self, DomainError> {
        if let Some(scope) = source_scope.as_deref() {
            SourceScope::parse(scope)?;
        }
        let source = Self {
            id: required_text("id", id)?,
            topic: required_text("topic", topic)?,
            kind,
            uri: required_text("uri", uri)?,
            source_scope,
            read_policy: "direct".to_owned(),
            write_policy: "manual-review".to_owned(),
            status: "active".to_owned(),
            version: 1,
            description,
        };
        source.validate()?;
        Ok(source)
    }

    fn validate(&self) -> Result<(), DomainError> {
        required_text("id", self.id.as_str())?;
        required_text("topic", self.topic.as_str())?;
        required_text("uri", self.uri.as_str())?;
        required_text("read_policy", self.read_policy.as_str())?;
        required_text("write_policy", self.write_policy.as_str())?;
        required_text("status", self.status.as_str())?;
        if self.version == 0 {
            return Err(DomainError::invalid("version", "must be greater than zero"));
        }
        if let Some(scope) = self.source_scope.as_deref() {
            SourceScope::parse(scope)?;
        }
        Ok(())
    }
}

/// Supported source category labels in the YAML contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeMapSourceKind {
    Repo,
    File,
    Doc,
    Config,
    Db,
    Ci,
    Runtime,
    Wiki,
    Monitoring,
}

/// Ordered source route for a topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeMapRoute {
    pub topic: String,
    #[serde(default)]
    pub source_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

impl KnowledgeMapRoute {
    fn validate(&self) -> Result<(), DomainError> {
        required_text("topic", self.topic.as_str())?;
        for source_id in &self.source_order {
            required_text("source_order", source_id.as_str())?;
        }
        Ok(())
    }
}

/// Version history entry written after CLI mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeMapHistoryEntry {
    pub version: u64,
    pub action: String,
    pub actor: String,
    pub summary: String,
}

impl KnowledgeMapHistoryEntry {
    fn validate(&self) -> Result<(), DomainError> {
        if self.version == 0 {
            return Err(DomainError::invalid(
                "history",
                "version must be greater than zero",
            ));
        }
        required_text("action", self.action.as_str())?;
        required_text("actor", self.actor.as_str())?;
        required_text("summary", self.summary.as_str())?;
        Ok(())
    }
}

/// Optional source changes accepted by the update command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMapChange {
    pub id: String,
    pub topic: Option<String>,
    pub kind: Option<KnowledgeMapSourceKind>,
    pub uri: Option<String>,
    pub source_scope: Option<String>,
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_source_and_route() {
        let mut map = KnowledgeMap::initial("now".to_owned());
        map.add_source(
            KnowledgeMapSource::new(
                "build-cargo".to_owned(),
                "build".to_owned(),
                KnowledgeMapSourceKind::Config,
                "Cargo.toml".to_owned(),
                Some("repo".to_owned()),
                None,
            )
            .expect("source should parse"),
        )
        .expect("source should add");

        assert_eq!(map.topics[0].id, "build");
        assert_eq!(map.routes[0].source_order, ["build-cargo"]);
        map.validate().expect("map should validate");
    }

    #[test]
    fn keeps_multiple_sources_under_one_topic() {
        let mut map = KnowledgeMap::initial("now".to_owned());
        for (id, uri) in [
            (
                "cli-reference",
                "docs/zh/01-user-guide/03-cli-command-reference.md",
            ),
            (
                "cli-skill",
                "skills/relay-knowledge-cli/references/knowledge-map-workflows.md",
            ),
        ] {
            map.add_source(
                KnowledgeMapSource::new(
                    id.to_owned(),
                    "cli".to_owned(),
                    KnowledgeMapSourceKind::Doc,
                    uri.to_owned(),
                    Some("docs".to_owned()),
                    None,
                )
                .expect("source should parse"),
            )
            .expect("source should add");
        }

        assert_eq!(
            map.routes[0].source_order,
            ["cli-reference".to_owned(), "cli-skill".to_owned()]
        );
        assert_eq!(
            map.sources
                .iter()
                .filter(|source| source.topic == "cli")
                .count(),
            2
        );
    }

    #[test]
    fn moving_source_prunes_old_topic_route() {
        let mut map = KnowledgeMap::initial("now".to_owned());
        map.add_source(
            KnowledgeMapSource::new(
                "shared-doc".to_owned(),
                "build".to_owned(),
                KnowledgeMapSourceKind::Doc,
                "docs/build.md".to_owned(),
                None,
                None,
            )
            .expect("source should parse"),
        )
        .expect("source should add");

        map.update_source(KnowledgeMapChange {
            id: "shared-doc".to_owned(),
            topic: Some("cli".to_owned()),
            kind: None,
            uri: None,
            source_scope: None,
            description: None,
        })
        .expect("source should move");

        assert!(
            map.routes
                .iter()
                .find(|route| route.topic == "build")
                .is_none_or(|route| route.source_order.is_empty())
        );
        assert_eq!(
            map.routes
                .iter()
                .find(|route| route.topic == "cli")
                .expect("new route should exist")
                .source_order,
            ["shared-doc".to_owned()]
        );
    }

    #[test]
    fn rejects_duplicate_sources_and_bad_routes() {
        let mut map = KnowledgeMap::initial("now".to_owned());
        let source = KnowledgeMapSource::new(
            "docs".to_owned(),
            "architecture".to_owned(),
            KnowledgeMapSourceKind::Doc,
            "docs/README.md".to_owned(),
            None,
            None,
        )
        .expect("source should parse");
        map.add_source(source.clone())
            .expect("first add should work");
        assert!(map.add_source(source).is_err());

        map.routes[0].source_order.push("missing".to_owned());
        assert!(map.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_route_topics() {
        let mut map = routed_map();
        map.routes.push(KnowledgeMapRoute {
            topic: "architecture".to_owned(),
            source_order: Vec::new(),
            fallback: None,
        });

        let error = map.validate().expect_err("duplicate route should fail");

        assert!(error.to_string().contains("route topics must be unique"));
    }

    #[test]
    fn rejects_duplicate_sources_inside_route_order() {
        let mut map = routed_map();
        map.routes[0].source_order.push("docs".to_owned());

        let error = map
            .validate()
            .expect_err("duplicate route source should fail");

        assert!(error.to_string().contains("repeats source 'docs'"));
    }

    #[test]
    fn rejects_unrouted_sources() {
        let mut map = routed_map();
        map.sources.push(
            KnowledgeMapSource::new(
                "unrouted".to_owned(),
                "architecture".to_owned(),
                KnowledgeMapSourceKind::Doc,
                "docs/unrouted.md".to_owned(),
                None,
                None,
            )
            .expect("source should parse"),
        );

        let error = map.validate().expect_err("unrouted source should fail");

        assert!(
            error
                .to_string()
                .contains("source 'unrouted' is not routed")
        );
    }

    #[test]
    fn rejects_invalid_history_contracts() {
        let mut map = routed_map();
        map.history[0].summary.clear();
        assert!(map.validate().is_err());

        let mut map = routed_map();
        map.history.push(KnowledgeMapHistoryEntry {
            version: 3,
            action: "source.add".to_owned(),
            actor: "cli".to_owned(),
            summary: "Skipped a version.".to_owned(),
        });
        map.map_version = 3;
        let error = map.validate().expect_err("skipped history should fail");
        assert!(error.to_string().contains("contiguous"));

        let mut map = routed_map();
        map.map_version = 2;
        let error = map
            .validate()
            .expect_err("mismatched map version should fail");
        assert!(error.to_string().contains("must match map_version 2"));
    }

    fn routed_map() -> KnowledgeMap {
        let mut map = KnowledgeMap::initial("now".to_owned());
        map.add_source(
            KnowledgeMapSource::new(
                "docs".to_owned(),
                "architecture".to_owned(),
                KnowledgeMapSourceKind::Doc,
                "docs/README.md".to_owned(),
                None,
                None,
            )
            .expect("source should parse"),
        )
        .expect("source should add");
        map
    }
}
