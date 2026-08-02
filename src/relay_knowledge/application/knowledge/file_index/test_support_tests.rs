use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    application::{RelayKnowledgeService, RuntimeConfiguration},
    env::{EnvironmentConfig, PlatformKind},
    storage::{KnowledgeStore, SqliteGraphStore},
};

pub(super) struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    pub(super) fn new(name: &str) -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("fixture root should be created");

        Self { root }
    }

    pub(super) fn path(&self) -> &Path {
        &self.root
    }

    pub(super) fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(path, content).expect("fixture file should be written");
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) async fn service_for_root(root: &Path) -> RelayKnowledgeService {
    let home = root.join("home");
    fs::create_dir_all(&home).expect("home should be created");
    let relay_home = root.join("relay");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", home.to_string_lossy().to_string()),
            ("TMPDIR", "/tmp".to_owned()),
            (
                "RELAY_KNOWLEDGE_HOME",
                relay_home.to_string_lossy().to_string(),
            ),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS",
                root.to_string_lossy().to_string(),
            ),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"))
        as Arc<dyn KnowledgeStore>;

    RelayKnowledgeService::with_store(runtime, store)
}
