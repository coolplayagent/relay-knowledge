use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

pub(super) fn context(name: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{name}"),
        format!("trace-{name}"),
    )
}

pub(super) async fn service_with_store(store: Arc<SqliteGraphStore>) -> RelayKnowledgeService {
    let environment = test_environment();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("test runtime should compose");
    RelayKnowledgeService::with_store(runtime, store)
}

#[cfg(windows)]
fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Windows,
        [
            ("USERPROFILE", "C:\\Users\\alice"),
            ("APPDATA", "C:\\Users\\alice\\AppData\\Roaming"),
            ("LOCALAPPDATA", "C:\\Users\\alice\\AppData\\Local"),
            ("TEMP", "C:\\Users\\alice\\AppData\\Local\\Temp"),
            ("RELAY_KNOWLEDGE_HOME", "C:\\relay"),
        ],
    )
    .expect("test environment should parse")
}

#[cfg(not(windows))]
fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("test environment should parse")
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
            fs::create_dir_all(parent).expect("fixture parent should exist");
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
        assert!(output.status.success(), "git command should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}
