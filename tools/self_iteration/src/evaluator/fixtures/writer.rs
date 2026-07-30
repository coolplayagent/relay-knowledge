use std::{fs, path::Path};

pub(in crate::evaluator) fn write_fixture_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(path, content).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

#[cfg(test)]
#[path = "writer_tests.rs"]
mod writer_tests;
