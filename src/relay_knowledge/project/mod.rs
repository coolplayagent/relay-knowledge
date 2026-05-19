//! Project identity shared by public API responses and adapters.

/// Canonical package and product name used in user-facing output.
pub const PROJECT_NAME: &str = "relay-knowledge";

/// Canonical application directory name used in platform runtime paths.
pub const APP_DIR_NAME: &str = PROJECT_NAME;

/// Default SQLite database filename stored under the resolved data directory.
pub const DATABASE_FILE_NAME: &str = "relay-knowledge.sqlite";

/// Model provider profile configuration filename stored under the config directory.
pub const MODEL_PROFILES_FILE_NAME: &str = "model-profiles.json";

/// Model provider fallback policy filename stored under the config directory.
pub const MODEL_FALLBACK_FILE_NAME: &str = "model-fallback.json";

/// Model catalog cache filename stored under the cache directory.
pub const MODEL_CATALOG_CACHE_FILE_NAME: &str = "model-catalog-cache.json";

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

/// Resident MCP adapter identity reported in unified API metadata.
pub const MCP_ADAPTER_NAME: &str = "relay-knowledge-mcp";

/// Local ACP adapter identity reported in unified API metadata.
pub const ACP_LOCAL_ADAPTER_NAME: &str = "relay-knowledge-acp-local";
