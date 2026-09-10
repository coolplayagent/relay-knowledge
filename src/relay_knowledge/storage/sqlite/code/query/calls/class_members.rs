//! Resolve Java class names to bounded, directly owned callable records.

use rusqlite::{Connection, params};

use super::super::{prepare_code_search_statement, relevance::SymbolIdentityQuery};
use crate::storage::StorageError;

const MAX_CLASSES: usize = 64;
const MAX_MEMBERS: usize = 1024;

pub(super) struct ClassMember {
    pub(super) name: String,
    pub(super) snapshot: String,
}

pub(super) fn resolve(
    connection: &Connection,
    scope: &str,
    identity: &SymbolIdentityQuery,
) -> Result<Option<Vec<ClassMember>>, StorageError> {
    let mut statement = prepare_code_search_statement(
        connection,
        "SELECT symbol_snapshot_id, qualified_name, path, byte_start, byte_end
         FROM code_repository_symbols
         WHERE source_scope = ?1 AND name = ?2 AND kind = 'class' AND language_id = 'java'
           AND (?3 IS NULL OR lower(qualified_name) LIKE ?3 ESCAPE '\\')
         LIMIT ?4",
    )?;
    let mut rows = statement.query(params![
        scope,
        identity.leaf_name(),
        identity.scoped_like_pattern(),
        (MAX_CLASSES + 1) as i64,
    ])?;
    let mut members = Vec::new();
    let mut matched = false;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > MAX_CLASSES {
            return Err(capacity("more than 64 candidate classes"));
        }
        let owner: String = row.get(1)?;
        if !identity.matches_symbol(identity.leaf_name(), &owner, "", "") {
            continue;
        }
        matched = true;
        let mut statement = prepare_code_search_statement(
            connection,
            "SELECT name, symbol_snapshot_id FROM code_repository_symbols
             WHERE source_scope = ?1 AND path = ?2 AND language_id = 'java'
               AND byte_start >= ?3 AND byte_end <= ?4
               AND (symbol_snapshot_id = ?5 OR (
                   kind IN ('method', 'constructor', 'function', 'function_declaration')
                   AND substr(qualified_name, 1, length(?6) + 1) = ?6 || '.'
                   AND instr(substr(qualified_name, length(?6) + 2), '.') = 0))
             LIMIT ?7",
        )?;
        let mut member_rows = statement.query(params![
            scope,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, String>(0)?,
            owner,
            (MAX_MEMBERS + 1 - members.len()) as i64,
        ])?;
        while let Some(member) = member_rows.next()? {
            if members.len() == MAX_MEMBERS {
                return Err(capacity("more than 1024 class/member records"));
            }
            members.push(ClassMember {
                name: member.get(0)?,
                snapshot: member.get(1)?,
            });
        }
    }
    Ok(matched.then_some(members))
}

pub(super) fn capacity(reason: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "class call query incomplete: {reason}; use a qualified class name or query a member method"
    ))
}

#[cfg(test)]
#[path = "class_members_tests.rs"]
mod tests;
