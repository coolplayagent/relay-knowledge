use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    code::{CodeIndexError, source::changes::GitTreeEntry},
    domain::{CodeRepositoryRegistration, CodeRepositorySelector},
};

use super::{
    filesystem_policy_for_selector, scoped_filesystem_tree_hash, scoped_source_snapshot_for_filters,
};

#[test]
fn filesystem_policy_denies_disjoint_registration_and_selector_scopes() {
    let registration = registration(vec!["src".to_owned()]);
    let selector =
        CodeRepositorySelector::new("alias", "HEAD", vec!["tests".to_owned()], Vec::new())
            .expect("selector should validate");

    let policy = filesystem_policy_for_selector(&registration, &selector);

    assert!(policy.path_scope_denied);
    assert!(policy.path_scope_filters.is_empty());
}

#[test]
fn scoped_filesystem_snapshot_keeps_selected_content_and_hashes_aligned() {
    let source = TestSource::create("scoped-snapshot");
    source.write("src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
    source.write("tests/lib.rs", "#[test]\nfn answers() {}\n");

    let snapshot =
        scoped_source_snapshot_for_filters(source.path(), "HEAD", &["src".to_owned()], &[])
            .expect("filesystem snapshot should resolve");

    assert_eq!(
        snapshot
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        vec!["src/lib.rs"]
    );
    assert_eq!(
        snapshot
            .content_hashes
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["src/lib.rs"]
    );
    assert_eq!(snapshot.resolved_commit_sha, snapshot.tree_hash);
    assert_eq!(snapshot.path_filters, vec!["src"]);
}

#[test]
fn scoped_filesystem_tree_hash_rejects_a_stale_snapshot_ref() {
    let source = TestSource::create("stale-scoped-snapshot");
    source.write("src/lib.rs", "pub fn initial() {}\n");
    let entries = vec![GitTreeEntry {
        path: "src/lib.rs".to_owned(),
        byte_count: 20,
    }];
    let (_, tree_hash, _) = scoped_filesystem_tree_hash(source.path(), &entries, "HEAD")
        .expect("live filesystem hash should resolve");
    source.write("src/lib.rs", "pub fn changed() {}\n");

    let error = scoped_filesystem_tree_hash(source.path(), &entries, &tree_hash)
        .expect_err("stale filesystem ref should be rejected");

    assert!(
        matches!(error, CodeIndexError::InvalidInput(message) if message.contains(
            "no longer matches live indexed scope"
        ))
    );
}

fn registration(path_filters: Vec<String>) -> CodeRepositoryRegistration {
    CodeRepositoryRegistration::new("repo", "alias", "/tmp/repository", path_filters, Vec::new())
        .expect("registration should validate")
}

struct TestSource {
    path: PathBuf,
}

impl TestSource {
    fn create(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "relay-knowledge-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("source root should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("source parent should be created");
        }
        fs::write(path, content).expect("source file should be written");
    }
}

impl Drop for TestSource {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
