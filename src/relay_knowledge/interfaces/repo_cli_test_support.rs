use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    api::{InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

pub(super) fn json_value(output: &str) -> serde_json::Value {
    serde_json::from_str(output.trim()).expect("json output should parse")
}

pub(super) fn context(name: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{name}"),
        format!("trace-{name}"),
    )
}

pub(super) async fn service_with_memory_store() -> RelayKnowledgeService {
    let temp_dir = std::env::temp_dir();
    let home_dir = temp_dir.join("relay-knowledge-test-user-home");
    let relay_home = temp_dir.join("relay-knowledge-test-runtime-home");
    let temp_dir = temp_dir.to_string_lossy().into_owned();
    let home_dir = home_dir.to_string_lossy().into_owned();
    let relay_home = relay_home.to_string_lossy().into_owned();
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [
            ("HOME", home_dir),
            ("TMPDIR", temp_dir.clone()),
            ("TEMP", temp_dir.clone()),
            ("TMP", temp_dir),
            ("RELAY_KNOWLEDGE_HOME", relay_home),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    RelayKnowledgeService::with_store(runtime, store)
}

pub(super) struct FixtureRepo {
    pub(super) path: PathBuf,
}

impl FixtureRepo {
    pub(super) fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(path.join("src")).expect("repo directory should be created");
        let repo = Self { path };
        repo.git(["init"]);
        repo.git(["config", "user.email", "relay@example.invalid"]);
        repo.git(["config", "user.name", "Relay Test"]);
        repo
    }

    pub(super) fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    pub(super) fn git<const N: usize>(&self, args: [&str; N]) {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(super) fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

impl Drop for FixtureRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}
