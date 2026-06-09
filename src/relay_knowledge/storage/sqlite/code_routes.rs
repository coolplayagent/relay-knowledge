use rusqlite::{Transaction, params};

use crate::{domain::CodeRouteRecord, storage::StorageError};

use super::SearchDocumentInserter;

const ROUTE_INTENT_SEARCH_TERMS: &str = "route endpoint http";

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    records: &[CodeRouteRecord],
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_routes (
            repository_id, source_scope, route_id, file_id, path, language_id,
            url, http_method, handler_name, handler_symbol_snapshot_id, framework,
            line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for record in records {
        statement.execute(params![
            record.repository_id,
            record.source_scope,
            record.route_id,
            record.file_id,
            record.path,
            record.language_id,
            record.url,
            record.http_method,
            record.handler_name,
            record.handler_symbol_snapshot_id,
            record.framework,
            record.line_range.start,
            record.line_range.end,
        ])?;
        let http_method_terms = route_http_method_search_terms(&record.http_method);
        let handler_terms = route_handler_search_terms(&record.handler_name);
        search_documents.insert(
            &record.source_scope,
            "route",
            &record.route_id,
            &record.path,
            &record.language_id,
            [
                ROUTE_INTENT_SEARCH_TERMS,
                record.url.as_str(),
                http_method_terms.as_str(),
                record.handler_name.as_str(),
                handler_terms.as_str(),
                record.framework.as_str(),
                record.path.as_str(),
            ],
        )?;
    }

    Ok(())
}

fn route_http_method_search_terms(method: &str) -> String {
    if method == "any" {
        return "any get post put delete patch head options".to_owned();
    }
    method.to_owned()
}

fn route_handler_search_terms(handler_name: &str) -> String {
    let mut terms = Vec::new();
    for token in handler_name
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
    {
        terms.push(token.to_ascii_lowercase());
        terms.extend(
            token
                .split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        terms.extend(route_handler_camel_case_terms(token));
    }
    terms.sort();
    terms.dedup();

    terms.join(" ")
}

fn route_handler_camel_case_terms(token: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let chars = token.char_indices().collect::<Vec<_>>();
    for (index, (byte_index, character)) in chars.iter().enumerate() {
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if *byte_index > start && starts_upper_word {
            terms.push(token[start..*byte_index].to_ascii_lowercase());
            start = *byte_index;
        }
        previous = Some(*character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }

    terms
}

#[cfg(test)]
mod tests {
    use super::{route_handler_search_terms, route_http_method_search_terms};

    #[test]
    fn any_route_method_search_terms_include_concrete_verbs() {
        let terms = route_http_method_search_terms("any");

        assert!(terms.contains("any"));
        assert!(terms.contains("get"));
        assert!(terms.contains("post"));
        assert!(terms.contains("options"));
    }

    #[test]
    fn route_handler_search_terms_split_identifier_parts() {
        let terms = route_handler_search_terms("usersController.listActiveUsers");

        assert!(terms.contains("users"));
        assert!(terms.contains("controller"));
        assert!(terms.contains("list"));
        assert!(terms.contains("active"));
    }
}
