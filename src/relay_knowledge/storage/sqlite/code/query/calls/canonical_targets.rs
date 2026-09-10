//! Scope-bounded canonical selection and structured declaration equivalence.

use super::super::prepare_code_search_statement;
use crate::{
    domain::{
        MAX_CALLABLE_SIGNATURE_KEY_BYTES,
        code_call_targets::{callable_definition_symbol, callable_target_symbol_kind},
    },
    storage::StorageError,
};
use rusqlite::{Connection, types::ValueRef};

const MAX_CANONICAL_SYMBOL_CANDIDATES: usize = 1024;

struct Candidate {
    snapshot: String,
    declaration: bool,
    c_family: bool,
    key: Option<String>,
}

pub(super) fn snapshots(
    connection: &Connection,
    scope: &str,
    canonical_id: &str,
    include_declarations: bool,
) -> Result<Vec<String>, StorageError> {
    let mut statement = prepare_code_search_statement(connection,
        "SELECT symbol_snapshot_id, kind, signature, language_id, callable_signature_key
         FROM code_repository_symbols WHERE source_scope = ?1 AND canonical_symbol_id = ?2 LIMIT ?3")?;
    let mut rows = statement.query(rusqlite::params![
        scope,
        canonical_id,
        (MAX_CANONICAL_SYMBOL_CANDIDATES + 1) as i64
    ])?;
    let mut candidates = Vec::new();
    let mut definition = None;
    let mut declaration = None;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > MAX_CANONICAL_SYMBOL_CANDIDATES {
            return Err(ambiguous(
                "canonical symbol exceeds the 1024-candidate definition budget",
            ));
        }
        let kind: String = row.get(1)?;
        if !callable_target_symbol_kind(&kind) {
            continue;
        }
        let signature: String = row.get(2)?;
        let is_definition = callable_definition_symbol(&kind, &signature);
        if is_definition {
            if definition.is_some() {
                return Err(ambiguous(
                    "canonical symbol matches multiple definitions in this scope",
                ));
            }
            definition = Some(candidates.len());
        } else if declaration.is_none() {
            declaration = Some(candidates.len());
        }
        let language: String = row.get(3)?;
        let c_family = matches!(language.as_str(), "c" | "cpp");
        let key = if c_family && include_declarations {
            match row.get_ref(4)? {
                ValueRef::Null => None,
                ValueRef::Text(bytes)
                    if bytes.len() <= MAX_CALLABLE_SIGNATURE_KEY_BYTES
                        && bytes.starts_with(b"c-family-callable-v1|") =>
                {
                    Some(
                        std::str::from_utf8(bytes)
                            .map_err(|error| StorageError::InvalidInput(error.to_string()))?
                            .to_owned(),
                    )
                }
                _ => {
                    return Err(StorageError::InvalidInput(
                        "canonical callable signature evidence is invalid or exceeds its byte budget".into(),
                    ));
                }
            }
        } else {
            None
        };
        candidates.push(Candidate {
            snapshot: row.get(0)?,
            declaration: !is_definition,
            c_family,
            key,
        });
    }
    let Some(selected) = definition.or(declaration) else {
        return Ok(Vec::new());
    };
    if definition.is_none() && candidates.len() > 1 {
        return Err(ambiguous(
            "canonical symbol matches multiple callable declarations without a definition",
        ));
    }
    let selected = &candidates[selected];
    let mut result = vec![selected.snapshot.clone()];
    if !include_declarations || selected.declaration || !selected.c_family {
        return Ok(result);
    }
    for candidate in &candidates {
        if !candidate.declaration || !candidate.c_family {
            continue;
        }
        let (Some(selected_key), Some(candidate_key)) = (&selected.key, &candidate.key) else {
            return Err(ambiguous(
                "canonical declarations lack complete structured callable signatures; reindex legacy scopes or select an explicit snapshot",
            ));
        };
        if selected_key == candidate_key {
            result.push(candidate.snapshot.clone());
        }
    }
    Ok(result)
}

fn ambiguous(reason: &str) -> StorageError {
    StorageError::AmbiguousCodeSymbol(format!(
        "{reason}; use the desired callable's symbol_snapshot_id (symbol:...) as --query"
    ))
}

#[cfg(test)]
#[path = "canonical_targets_tests.rs"]
mod tests;
