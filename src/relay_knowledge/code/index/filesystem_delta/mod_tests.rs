// Direct tests for filesystem delta detection.

use super::{changed_paths_for_filesystem_diff, filesystem_content_hashes_for_paths};
use crate::code::test_fixtures::TempSourceDir;

#[test]
fn filesystem_provider_add_delete_reparses_unchanged_python_symbols() {
    let source = TempSourceDir::create("filesystem-python-origin");
    source.write(
        "app.py",
        "import typing\n@typing.overload\ndef pick(x:int): ...\ndef pick(x): return x\n",
    );
    let registration = source.registration();
    source.write(
        "window.pyw",
        "import typing\n@typing.overload\ndef pick(x:int): ...\ndef pick(x): return x\n",
    );
    let selector = source.selector();
    let first = crate::code::index::full_snapshot::build_full_snapshot(
        &registration,
        &selector,
        &source.path,
        &Default::default(),
    )
    .unwrap();
    let hashes = first
        .files
        .iter()
        .map(|file| (file.path.clone(), file.blob_hash.clone()))
        .collect();
    source.write("typing.py", "def overload(f): return f\n");
    let added = super::build_filesystem_delta_snapshot(
        &registration,
        &selector,
        &source.path,
        "HEAD",
        &hashes,
        Some(&first.resolved_commit_sha),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        added
            .symbols
            .iter()
            .filter(|s| s.name == "pick" && s.kind == "function")
            .count(),
        4
    );
    let paths = vec!["app.py".into(), "window.pyw".into(), "typing.py".into()];
    let hashes = filesystem_content_hashes_for_paths(&source.path, &paths).unwrap();
    std::fs::remove_file(source.path.join("typing.py")).unwrap();
    let removed = super::build_filesystem_delta_snapshot(
        &registration,
        &selector,
        &source.path,
        "HEAD",
        &hashes,
        Some(&added.resolved_commit_sha),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        removed
            .symbols
            .iter()
            .filter(|s| s.name == "pick" && s.kind == "function_declaration")
            .count(),
        2
    );
    assert!(removed.deleted_paths.iter().any(|path| path == "typing.py"));
}

#[test]
fn filesystem_diff_reports_deleted_base_paths() {
    let source = TempSourceDir::create("filesystem-diff-deletion");
    source.write("src/lib.rs", "pub fn unchanged() {}\n");
    source.write("src/api.rs", "pub fn removed() {}\n");
    let paths = vec!["src/api.rs".to_owned(), "src/lib.rs".to_owned()];
    let previous_hashes = filesystem_content_hashes_for_paths(&source.path, &paths)
        .expect("base filesystem hashes should compute");
    std::fs::remove_file(source.path.join("src/api.rs")).expect("indexed file should delete");

    let changed_paths =
        changed_paths_for_filesystem_diff(&source.path, "HEAD", &[], &[], &previous_hashes)
            .expect("filesystem diff should compare against stored base hashes");

    assert_eq!(changed_paths, ["src/api.rs".to_owned()]);
}

#[test]
fn origin_plan_counts_forced_consumers_and_changed_other_files_together() {
    use crate::code::changes::GitTreeEntry;
    use std::collections::BTreeMap;
    let limit = crate::domain::CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH;
    let previous = BTreeMap::from([
        ("app.pyw".into(), "same".into()),
        ("notes.txt".into(), "same".into()),
    ]);
    let mut planned = previous.clone();
    planned.insert("typing.py".into(), "provider".into());
    let mut entries = vec![
        GitTreeEntry {
            path: "app.pyw".into(),
            byte_count: limit - 1,
        },
        GitTreeEntry {
            path: "typing.py".into(),
            byte_count: 1,
        },
        GitTreeEntry {
            path: "notes.txt".into(),
            byte_count: 1,
        },
    ];
    assert!(super::validate_origin_plan(&entries, &previous, &planned).is_ok());
    planned.insert("notes.txt".into(), "changed".into());
    assert!(super::validate_origin_plan(&entries, &previous, &planned).is_err());
    entries = (0..513)
        .map(|i| GitTreeEntry {
            path: format!("app{i}.py"),
            byte_count: 0,
        })
        .collect();
    assert!(
        super::validate_origin_plan(&entries[..512], &BTreeMap::new(), &BTreeMap::new()).is_ok()
    );
    assert!(super::validate_origin_plan(&entries, &BTreeMap::new(), &BTreeMap::new()).is_err());
}

#[test]
fn real_filesystem_origin_refresh_rejects_aggregate_files_before_parsing() {
    let source = TempSourceDir::create("filesystem-origin-file-budget");
    let app = "import typing\n@typing.overload\ndef pick(x:int): ...\ndef pick(x): return x\n";
    let mut paths = Vec::new();
    for index in 0..512 {
        let path = format!("app{index}.py");
        source.write(&path, app);
        paths.push(path);
    }
    let hashes = filesystem_content_hashes_for_paths(&source.path, &paths).unwrap();
    source.write("typing.py", "def overload(f): return f\n");
    let error = super::build_filesystem_delta_snapshot(
        &source.registration(),
        &source.selector(),
        &source.path,
        "HEAD",
        &hashes,
        Some(&super::filesystem_tree_hash_from_path_hashes(&hashes)),
        &Default::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("bounded file/byte budget"),
        "{error}"
    );
    assert_eq!(hashes.len(), 512);
}
