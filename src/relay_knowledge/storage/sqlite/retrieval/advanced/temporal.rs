use rusqlite::Connection;

use crate::{
    domain::RetrieverSource,
    storage::{GraphSearchRequest, StorageError},
};

use super::{
    event::{load_events, occurred_label},
    support::SupportContext,
};
use crate::storage::sqlite::retrieval::{
    ScoredHit,
    local_model::{overlap_score, token_signature},
    sort_scored_hits,
};

pub(in crate::storage::sqlite::retrieval) fn temporal_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let temporal = TemporalQuery::parse(&request.query);
    if !temporal.requested {
        return Ok(Vec::new());
    }

    let mut hits = Vec::new();
    for event in load_events(connection, request)? {
        if !temporal.matches(event.occurred_at.as_deref()) {
            continue;
        }
        let Some(context) = SupportContext::load(connection, &event.evidence_ids_json, request)?
        else {
            continue;
        };
        let text = format!(
            "{} {} {} {}",
            event.event_type,
            event.occurred_at.as_deref().unwrap_or_default(),
            event.labels,
            context.content
        );
        let score = 1.0
            + overlap_score(
                &request.query,
                &text,
                &context.entity_labels,
                context.source_path.as_deref(),
            );
        let occurred = occurred_label(event.occurred_at.as_deref());
        let content = format!(
            "temporal event {}{}: {}\n{}",
            event.event_type, occurred, event.labels, context.content
        );
        let graph_fact = event.graph_fact(&context)?;
        hits.push(context.scored(
            content,
            RetrieverSource::Temporal,
            score,
            format!("temporal event {} matched query time constraints", event.id),
            Some(graph_fact),
        ));
    }
    sort_scored_hits(&mut hits);

    Ok(hits)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TemporalQuery {
    requested: bool,
    as_of: Option<String>,
    as_of_date: Option<TemporalDate>,
    time_terms: Vec<String>,
}

impl TemporalQuery {
    fn parse(query: &str) -> Self {
        let lowered = query.to_ascii_lowercase();
        let scrubbed_query = query
            .split_whitespace()
            .filter(|token| strip_as_of_value(token).is_none())
            .collect::<Vec<_>>()
            .join(" ");
        let time_terms = token_signature(&scrubbed_query, &[], None)
            .into_iter()
            .filter(|term| term.len() == 4 && term.chars().all(|ch| ch.is_ascii_digit()))
            .collect::<Vec<_>>();
        let as_of = extract_as_of(query);
        let as_of_date = as_of.as_deref().and_then(TemporalDate::parse);
        let requested = as_of.is_some()
            || !time_terms.is_empty()
            || ["when", "timeline", "history", "temporal"]
                .iter()
                .any(|needle| lowered.contains(needle));

        Self {
            requested,
            as_of,
            as_of_date,
            time_terms,
        }
    }

    fn matches(&self, occurred_at: Option<&str>) -> bool {
        let Some(occurred_at) = occurred_at else {
            return false;
        };
        if self.time_terms.is_empty() && self.as_of.is_none() {
            return true;
        }
        if let Some(as_of) = self.as_of_date {
            let Some(occurred) = TemporalDate::parse(occurred_at) else {
                return false;
            };
            if !occurred.is_on_or_before(as_of) {
                return false;
            }
            return self.time_terms.is_empty()
                || self
                    .time_terms
                    .iter()
                    .any(|term| occurred_at.contains(term));
        }

        self.time_terms
            .iter()
            .any(|term| occurred_at.contains(term))
    }
}

fn extract_as_of(query: &str) -> Option<String> {
    query.split_whitespace().find_map(|token| {
        strip_as_of_value(token)
            .map(|value| {
                value
                    .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
                    .to_owned()
            })
            .filter(|value| !value.is_empty())
    })
}

fn strip_as_of_value(token: &str) -> Option<&str> {
    let lowered = token.to_ascii_lowercase();
    ["as_of:", "as-of:"]
        .iter()
        .find_map(|prefix| lowered.starts_with(prefix).then(|| &token[prefix.len()..]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TemporalDate {
    year: u16,
    month: Option<u8>,
    day: Option<u8>,
}

impl TemporalDate {
    fn parse(value: &str) -> Option<Self> {
        value.split_whitespace().find_map(|token| {
            let token = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '/');
            let token = token
                .split(|ch: char| !ch.is_ascii_digit() && ch != '-' && ch != '/')
                .next()
                .unwrap_or_default();
            let separator = if token.contains('-') { '-' } else { '/' };
            let parts = token.split(separator).collect::<Vec<_>>();
            let year = parts.first().copied()?;
            if year.len() != 4 || !year.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            let year = year.parse::<u16>().ok()?;
            let month = match parts.get(1).copied() {
                Some(value) => Some(parse_date_component(value)?),
                None => None,
            };
            let day = match parts.get(2).copied() {
                Some(value) => Some(parse_date_component(value)?),
                None => None,
            };
            if parts.len() > 3
                || month.is_some_and(|value| !(1..=12).contains(&value))
                || day.is_some_and(|value| !(1..=31).contains(&value))
            {
                return None;
            }

            Some(Self { year, month, day })
        })
    }

    fn is_on_or_before(self, cutoff: Self) -> bool {
        self.lower_bound() <= cutoff.upper_bound()
    }

    fn lower_bound(self) -> (u16, u8, u8) {
        (self.year, self.month.unwrap_or(1), self.day.unwrap_or(1))
    }

    fn upper_bound(self) -> (u16, u8, u8) {
        (self.year, self.month.unwrap_or(12), self.day.unwrap_or(31))
    }
}

fn parse_date_component(value: &str) -> Option<u8> {
    (!value.is_empty() && value.len() <= 2 && value.chars().all(|ch| ch.is_ascii_digit()))
        .then(|| value.parse::<u8>().ok())
        .flatten()
}

#[cfg(test)]
#[path = "temporal_tests.rs"]
mod tests;
