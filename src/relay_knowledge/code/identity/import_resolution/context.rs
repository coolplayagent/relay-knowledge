use std::collections::BTreeMap;

use crate::{
    code::source_roots::{
        c_family_module_candidates, go_module_candidates, source_module_candidates,
    },
    domain::{RepositoryCodeFileRecord, RepositoryCodeSymbolRecord},
};

pub(in crate::code::identity) struct ImportContext<'a> {
    pub(super) file_languages: BTreeMap<&'a str, &'a str>,
    pub(super) module_paths: BTreeMap<String, Vec<&'a RepositoryCodeFileRecord>>,
    pub(super) go_module_paths: BTreeMap<String, Vec<&'a RepositoryCodeFileRecord>>,
    pub(super) symbols_by_name: BTreeMap<&'a str, Vec<&'a RepositoryCodeSymbolRecord>>,
}

impl<'a> ImportContext<'a> {
    pub(in crate::code::identity) fn new(
        files: &'a [RepositoryCodeFileRecord],
        symbols: &'a [RepositoryCodeSymbolRecord],
    ) -> Self {
        let mut file_languages = BTreeMap::new();
        let mut module_paths = BTreeMap::<String, Vec<&RepositoryCodeFileRecord>>::new();
        let mut go_module_paths = BTreeMap::<String, Vec<&RepositoryCodeFileRecord>>::new();
        for file in files {
            file_languages.insert(file.path.as_str(), file.language_id.as_str());
            let candidates = if matches!(file.language_id.as_str(), "c" | "cpp") {
                c_family_module_candidates(&file.path)
            } else {
                source_module_candidates(&file.path)
            };
            for module_path in candidates {
                module_paths.entry(module_path).or_default().push(file);
            }
            if file.language_id == "go" {
                for module_path in go_module_candidates(&file.path) {
                    go_module_paths.entry(module_path).or_default().push(file);
                }
            }
        }

        let mut symbols_by_name = BTreeMap::<&str, Vec<&RepositoryCodeSymbolRecord>>::new();
        for symbol in symbols {
            symbols_by_name
                .entry(symbol.name.as_str())
                .or_default()
                .push(symbol);
        }

        Self {
            file_languages,
            module_paths,
            go_module_paths,
            symbols_by_name,
        }
    }

    pub(in crate::code::identity) fn language_for_path(&self, path: &str) -> Option<&'a str> {
        self.file_languages.get(path).copied()
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
