use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) struct GitRepositoryFixture {
    root: PathBuf,
}

impl GitRepositoryFixture {
    pub(super) fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-candidate-git-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("Git fixture root should be created");
        let fixture = Self { root };
        fixture.run(&["init", "--quiet"]);
        fixture.run(&["config", "user.name", "Relay Knowledge Test"]);
        fixture.run(&["config", "user.email", "relay-knowledge@example.invalid"]);
        fixture.write("tracked.txt", "initial\n");
        fixture.run(&["add", "tracked.txt"]);
        fixture.run(&["commit", "--quiet", "-m", "Initial fixture"]);
        fixture
    }

    pub(super) fn path(&self) -> &Path {
        &self.root
    }

    pub(super) fn write(&self, relative_path: &str, content: &str) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        std::fs::write(path, content).expect("fixture file should be written");
    }

    pub(super) fn read(&self, relative_path: &str) -> String {
        std::fs::read_to_string(self.root.join(relative_path))
            .expect("fixture file should be readable")
    }

    pub(super) fn run(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("Git fixture command should start");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("Git fixture output should be UTF-8")
    }

    pub(super) fn head(&self) -> String {
        self.run(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    pub(super) fn short_head(&self) -> String {
        self.run(&["rev-parse", "--short", "HEAD"])
            .trim()
            .to_owned()
    }

    pub(super) fn status(&self) -> String {
        self.run(&["status", "--porcelain"])
    }
}

impl Drop for GitRepositoryFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
