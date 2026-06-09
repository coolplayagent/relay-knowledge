//! Project identity shared by public API responses and adapters.

/// Canonical package and product name used in user-facing output.
pub const PROJECT_NAME: &str = "relay-knowledge";

/// Canonical application directory name used in platform runtime paths.
pub const APP_DIR_NAME: &str = PROJECT_NAME;

/// Default SQLite database filename stored under the resolved data directory.
pub const DATABASE_FILE_NAME: &str = "relay-knowledge.sqlite";

/// Directory under the data directory that stores pluggable storage backends.
pub const STORAGE_BACKENDS_DIR_NAME: &str = "stores";

/// Directory under the storage backends directory that stores repository shards.
pub const REPOSITORY_SHARDS_DIR_NAME: &str = "repositories";

/// SQLite filename used by each partitioned repository shard.
pub const REPOSITORY_SHARD_DATABASE_FILE_NAME: &str = "code.sqlite";

/// Model provider profile configuration filename stored under the config directory.
pub const MODEL_PROFILES_FILE_NAME: &str = "model-profiles.json";

/// Model provider fallback policy filename stored under the config directory.
pub const MODEL_FALLBACK_FILE_NAME: &str = "model-fallback.json";

/// Model catalog cache filename stored under the cache directory.
pub const MODEL_CATALOG_CACHE_FILE_NAME: &str = "model-catalog-cache.json";

/// Repository-relative directory that stores shared agent contracts.
pub const AGENT_CONTRACT_DIR_NAME: &str = ".knowledge";

/// Repository-relative knowledge navigation contract filename.
pub const KNOWLEDGE_MAP_FILE_NAME: &str = "knowledge-map.yaml";

/// Repository-relative knowledge navigation contract path referenced by agents.
pub const KNOWLEDGE_MAP_RELATIVE_PATH: &str = ".knowledge/knowledge-map.yaml";

/// Version-check cache filename stored under the cache directory.
pub const VERSION_CHECK_CACHE_FILE_NAME: &str = "version-check-cache.json";

/// Default GitHub repository queried for release metadata.
pub const GITHUB_REPOSITORY_FULL_NAME: &str = "coolplayagent/relay-knowledge";

/// Windows service definition filename for installed background operation.
pub const WINDOWS_SERVICE_DEFINITION_FILE_NAME: &str = "relay-knowledge-service.xml";

/// macOS launchd service definition filename for installed background operation.
pub const MACOS_SERVICE_DEFINITION_FILE_NAME: &str = "com.coolplayagent.relay-knowledge.plist";

/// Linux systemd service definition filename for installed background operation.
pub const LINUX_SERVICE_DEFINITION_FILE_NAME: &str = "relay-knowledge.service";

/// Lifecycle checkpoint filename stored beside generated service definitions.
pub const SERVICE_LIFECYCLE_CHECKPOINT_FILE_NAME: &str = "relay-knowledge-service-lifecycle.json";

/// Resident MCP adapter identity reported in unified API metadata.
pub const MCP_ADAPTER_NAME: &str = "relay-knowledge-mcp";

/// Local ACP adapter identity reported in unified API metadata.
pub const ACP_LOCAL_ADAPTER_NAME: &str = "relay-knowledge-acp-local";
