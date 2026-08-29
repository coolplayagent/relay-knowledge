use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[path = "architecture_boundaries/layout_contracts.rs"]
mod layout_contracts;
#[path = "architecture_boundaries/module_graph.rs"]
mod module_graph;

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/relay_knowledge")
}

fn production_rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_production_rust_files(root, &mut files);
    files.sort();
    files
}

fn collect_production_rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path)
        .unwrap_or_else(|error| panic!("read directory {}: {error}", path.display()))
    {
        let entry = entry.unwrap_or_else(|error| panic!("read directory entry: {error}"));
        let entry_path = entry.path();
        if entry_path.is_dir() {
            if entry_path.file_name().is_some_and(|name| name == "tests") {
                continue;
            }
            collect_production_rust_files(&entry_path, files);
        } else if entry_path
            .extension()
            .is_some_and(|extension| extension == "rs")
            && !is_test_support_file(&entry_path)
        {
            files.push(entry_path);
        }
    }
}

fn is_test_support_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name == "tests.rs"
                || name == "test_support.rs"
                || name.ends_with("_tests.rs")
                || name.ends_with("_test_support.rs")
        })
}

fn relative_source_path(path: &Path, source_root: &Path) -> String {
    let repository_root = source_root
        .parent()
        .and_then(Path::parent)
        .expect("source root has repository ancestors");
    path.strip_prefix(repository_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
