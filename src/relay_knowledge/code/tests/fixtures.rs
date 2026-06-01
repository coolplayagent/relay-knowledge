use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::domain::{
    CodeRepositoryRegistration, CodeRepositorySelector, RepositoryCodeRange,
    RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
};

pub(super) fn symbol(id: &str, path: &str, name: &str) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: "git_snapshot:test".to_owned(),
        symbol_snapshot_id: id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: format!("file-{id}"),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 8 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

pub(super) fn reference(id: &str, path: &str, name: &str) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: "git_snapshot:test".to_owned(),
        reference_id: id.to_owned(),
        file_id: format!("file-{id}"),
        path: path.to_owned(),
        name: name.to_owned(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some(name.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 8 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

pub(in crate::code) struct TempGitRepo {
    pub(super) path: PathBuf,
}

impl TempGitRepo {
    pub(in crate::code) fn create(name: &str) -> Self {
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

    pub(in crate::code) fn registration(&self) -> CodeRepositoryRegistration {
        CodeRepositoryRegistration::new(
            "repo",
            "alias",
            self.path.display().to_string(),
            vec!["src".to_owned()],
            vec!["rust".to_owned()],
        )
        .expect("registration should validate")
    }

    pub(super) fn selector(&self) -> CodeRepositorySelector {
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate")
    }

    pub(in crate::code) fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    pub(in crate::code) fn git<const N: usize>(&self, args: [&str; N]) {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(in crate::code) fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

impl Drop for TempGitRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(in crate::code) struct TempSourceDir {
    pub(super) path: PathBuf,
}

impl TempSourceDir {
    pub(in crate::code) fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(path.join("src")).expect("source directory should be created");

        Self { path }
    }

    pub(in crate::code) fn registration(&self) -> CodeRepositoryRegistration {
        CodeRepositoryRegistration::new(
            "repo",
            "alias",
            self.path.display().to_string(),
            Vec::new(),
            Vec::new(),
        )
        .expect("registration should validate")
    }

    pub(super) fn selector(&self) -> CodeRepositorySelector {
        CodeRepositorySelector::new("alias", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate")
    }

    pub(in crate::code) fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }
}

impl Drop for TempSourceDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
