//! Compact inverted export index used while rebuilding repository-set overlays.

use std::{cmp::Ordering, collections::BTreeMap};

use rusqlite::{Connection, params};

use crate::{domain::CodeRepositorySetMemberStatus, storage::StorageError};

use super::super::manifest::{
    manifest_module_prefixes_for_members, module_keys_for_path_with_prefixes,
    module_keys_for_symbol_path_with_prefixes, normalize_module_key,
};

pub(super) struct ExportTarget {
    pub(super) repository_id: String,
    pub(super) source_scope: String,
    pub(super) record_kind: String,
    pub(super) record_id: String,
}

pub(super) struct ExportIndex {
    targets: Vec<ExportTarget>,
    by_key: BTreeMap<String, Vec<usize>>,
}

impl ExportIndex {
    pub(super) fn for_members(
        connection: &mut Connection,
        members: &[CodeRepositorySetMemberStatus],
    ) -> Result<Self, StorageError> {
        let module_prefixes = manifest_module_prefixes_for_members(connection, members)?;
        let mut index = Self {
            targets: Vec::new(),
            by_key: BTreeMap::new(),
        };
        for member in members {
            let prefixes = module_prefixes
                .get(&member.member.source_scope)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let mut file_statement = connection.prepare(
                "
                SELECT repository_id, source_scope, file_id, path
                FROM code_repository_files
                WHERE source_scope = ?1
                ",
            )?;
            let mut file_rows = file_statement.query(params![member.member.source_scope])?;
            while let Some(row) = file_rows.next()? {
                let path = row.get::<_, String>(3)?;
                index.insert(
                    ExportTarget {
                        repository_id: row.get(0)?,
                        source_scope: row.get(1)?,
                        record_kind: "code_file".to_owned(),
                        record_id: row.get(2)?,
                    },
                    module_keys_for_path_with_prefixes(&path, prefixes),
                );
            }

            let mut symbol_statement = connection.prepare(
                "
                SELECT repository_id, source_scope, symbol_snapshot_id, name, qualified_name, path
                FROM code_repository_symbols
                WHERE source_scope = ?1
                ",
            )?;
            let mut symbol_rows = symbol_statement.query(params![member.member.source_scope])?;
            while let Some(row) = symbol_rows.next()? {
                let name = row.get::<_, String>(3)?;
                let qualified_name = row.get::<_, String>(4)?;
                let path = row.get::<_, String>(5)?;
                let mut keys = module_keys_for_symbol_path_with_prefixes(&path, prefixes);
                keys.insert(normalize_module_key(&name));
                keys.insert(normalize_module_key(&qualified_name));
                index.insert(
                    ExportTarget {
                        repository_id: row.get(0)?,
                        source_scope: row.get(1)?,
                        record_kind: "code_symbol_snapshot".to_owned(),
                        record_id: row.get(2)?,
                    },
                    keys,
                );
            }
        }

        Ok(index)
    }

    pub(super) fn matching_targets(&self, import_scope: &str, module: &str) -> Vec<&ExportTarget> {
        let exact = self.targets_for_key(module, import_scope);
        if !exact.is_empty() {
            return exact;
        }

        let Some((parent, imported_name)) = module.rsplit_once('.') else {
            return Vec::new();
        };
        if parent.is_empty() || imported_name.is_empty() {
            return Vec::new();
        }

        self.targets_for_key_intersection(parent, imported_name, import_scope)
    }

    fn insert(&mut self, target: ExportTarget, keys: impl IntoIterator<Item = String>) {
        let position = self.targets.len();
        self.targets.push(target);
        for key in keys {
            self.by_key.entry(key).or_default().push(position);
        }
    }

    fn targets_for_key(&self, key: &str, import_scope: &str) -> Vec<&ExportTarget> {
        self.by_key
            .get(key)
            .into_iter()
            .flatten()
            .filter_map(|position| self.target_for_import(*position, import_scope))
            .collect()
    }

    fn targets_for_key_intersection(
        &self,
        left_key: &str,
        right_key: &str,
        import_scope: &str,
    ) -> Vec<&ExportTarget> {
        let Some(left_positions) = self.by_key.get(left_key) else {
            return Vec::new();
        };
        let Some(right_positions) = self.by_key.get(right_key) else {
            return Vec::new();
        };

        let mut matches = Vec::new();
        let mut left = 0;
        let mut right = 0;
        while left < left_positions.len() && right < right_positions.len() {
            match left_positions[left].cmp(&right_positions[right]) {
                Ordering::Less => left += 1,
                Ordering::Greater => right += 1,
                Ordering::Equal => {
                    if let Some(target) = self.target_for_import(left_positions[left], import_scope)
                    {
                        matches.push(target);
                    }
                    left += 1;
                    right += 1;
                }
            }
        }

        matches
    }

    fn target_for_import(&self, position: usize, import_scope: &str) -> Option<&ExportTarget> {
        self.targets
            .get(position)
            .filter(|target| target.source_scope != import_scope)
    }
}

#[cfg(test)]
#[path = "export_index_tests.rs"]
mod tests;
