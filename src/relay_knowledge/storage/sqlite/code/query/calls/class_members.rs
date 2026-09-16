//! Resolve type names through persisted syntax ownership with bounded member reads.

use rusqlite::{Connection, params, params_from_iter, types::Value};
use std::collections::BTreeSet;

use super::super::prepare_code_search_statement;
use crate::storage::StorageError;

const MAX_CLASSES: usize = 64;
const MAX_MEMBERS: usize = 1024;

pub(super) struct ClassMember {
    pub(super) snapshot: String,
}

pub(super) fn resolve(
    connection: &Connection,
    scope: &str,
    name: &str,
    language_sql: &str,
    language_values: &[Value],
) -> Result<Option<Vec<ClassMember>>, StorageError> {
    let mut statement = prepare_code_search_statement(
        connection,
        &format!(
            "SELECT symbol_snapshot_id, type_owner_identity
         FROM code_repository_symbols
         WHERE source_scope = ?1 AND name = ?2 AND type_owner_json IS NOT NULL
           AND json_extract(type_owner_json, '$.relation') = 'declaration'
           AND coalesce(json_extract(type_owner_json, '$.resolution_state'), 'resolved')='resolved'
         {language_sql} LIMIT ?"
        ),
    )?;
    let mut values = vec![Value::Text(scope.to_owned()), Value::Text(name.to_owned())];
    values.extend_from_slice(language_values);
    values.push(Value::Integer((MAX_CLASSES + 1) as i64));
    let mut rows = statement.query(params_from_iter(values))?;
    let mut members = Vec::new();
    let mut matched = false;
    let mut count = 0;
    let mut owners = BTreeSet::new();
    while let Some(row) = rows.next()? {
        count += 1;
        if count > MAX_CLASSES {
            return Err(capacity("more than 64 candidate classes"));
        }
        let owner: String = row.get(1)?;
        matched = true;
        if !owners.insert(owner.clone()) {
            continue;
        }
        let mut statement = prepare_code_search_statement(
            connection,
            "SELECT symbol_snapshot_id FROM code_repository_symbols
             WHERE source_scope = ?1 AND type_owner_json IS NOT NULL
               AND type_owner_identity = ?2
               AND json_extract(type_owner_json, '$.relation') IN ('declaration', 'direct_member', 'trait_member')
               AND coalesce(json_extract(type_owner_json, '$.resolution_state'), 'resolved')='resolved'
             LIMIT ?3",
        )?;
        let mut member_rows = statement.query(params![
            scope,
            owner,
            (MAX_MEMBERS + 1 - members.len()) as i64,
        ])?;
        while let Some(member) = member_rows.next()? {
            if members.len() == MAX_MEMBERS {
                return Err(capacity("more than 1024 class/member records"));
            }
            members.push(ClassMember {
                snapshot: member.get(0)?,
            });
        }
    }
    if !matched && !language_sql.is_empty() {
        // An excluded type still selects the directional type-query contract;
        // it must not turn into a text/symbol fallback in another language.
        matched=connection.query_row("SELECT EXISTS(SELECT 1 FROM code_repository_symbols WHERE source_scope=?1 AND name=?2 AND json_extract(type_owner_json,'$.relation')='declaration')",params![scope,name],|row|row.get(0))?;
    }
    Ok(matched.then_some(members))
}

pub(super) fn capacity(reason: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "class call query incomplete: {reason}; query a member method"
    ))
}

#[cfg(test)]
#[path = "class_members_tests.rs"]
mod tests;
