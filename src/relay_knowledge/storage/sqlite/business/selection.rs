//! Business query eligibility, matching and result classification before pagination.
use crate::domain::{
    BusinessDomain, BusinessKnowledgeMatchType, BusinessKnowledgeQueryKind,
    BusinessKnowledgeQueryRequest, BusinessKnowledgeResult, BusinessKnowledgeResultStatus,
    BusinessTerm,
};
use std::collections::BTreeSet;

pub(super) fn select_terms(
    terms: &mut Vec<BusinessTerm>,
    domains: &[BusinessDomain],
    request: &BusinessKnowledgeQueryRequest,
    available: bool,
) -> BusinessKnowledgeResult {
    if !available {
        terms.clear();
    }
    if request.kind == BusinessKnowledgeQueryKind::Mappings {
        terms.retain(|term| !term.mappings.is_empty());
    }
    let match_type = filter_terms(terms, domains, request);
    let status = if !available {
        BusinessKnowledgeResultStatus::Unavailable
    } else if terms.is_empty() {
        BusinessKnowledgeResultStatus::NoMatch
    } else if match_type == Some(BusinessKnowledgeMatchType::Exact)
        && terms
            .iter()
            .map(|term| &term.domain_id)
            .collect::<BTreeSet<_>>()
            .len()
            > 1
    {
        BusinessKnowledgeResultStatus::Ambiguous
    } else {
        BusinessKnowledgeResultStatus::Matched
    };
    let truncated = terms.len() > request.limit;
    terms.truncate(request.limit);
    for term in terms.iter_mut() {
        match request.kind {
            BusinessKnowledgeQueryKind::Terms => term.mappings.clear(),
            BusinessKnowledgeQueryKind::Mappings => {
                term.definitions.clear();
                term.semantics.clear();
                term.conflicts.clear();
            }
            BusinessKnowledgeQueryKind::All => {}
        }
    }
    BusinessKnowledgeResult {
        status,
        match_type,
        returned_term_count: terms.len(),
        returned_mapping_count: terms.iter().map(|term| term.mappings.len()).sum(),
        truncated,
    }
}

fn filter_terms(
    terms: &mut Vec<BusinessTerm>,
    domains: &[BusinessDomain],
    request: &BusinessKnowledgeQueryRequest,
) -> Option<BusinessKnowledgeMatchType> {
    if let Some(domain) = &request.domain {
        let matching = domains
            .iter()
            .filter(|candidate| {
                candidate.id.eq_ignore_ascii_case(domain)
                    || candidate.name.eq_ignore_ascii_case(domain)
            })
            .map(|candidate| candidate.id.as_str())
            .collect::<BTreeSet<_>>();
        terms.retain(|term| matching.contains(term.domain_id.as_str()));
    }
    let query = request.query.as_ref()?;
    let exact = terms
        .iter()
        .filter(|term| {
            term.canonical_name.eq_ignore_ascii_case(query)
                || term
                    .aliases
                    .iter()
                    .any(|alias| alias.value.eq_ignore_ascii_case(query))
        })
        .map(|term| (term.domain_id.clone(), term.id.clone()))
        .collect::<BTreeSet<_>>();
    if !exact.is_empty() {
        terms.retain(|term| exact.contains(&(term.domain_id.clone(), term.id.clone())));
        return Some(BusinessKnowledgeMatchType::Exact);
    }
    let folded = query.to_lowercase();
    terms.retain(|term| {
        term.canonical_name.to_lowercase().contains(&folded)
            || term
                .aliases
                .iter()
                .any(|alias| alias.value.to_lowercase().contains(&folded))
            || term
                .definitions
                .iter()
                .any(|fact| fact.definition.to_lowercase().contains(&folded))
            || term
                .mappings
                .iter()
                .any(|mapping| mapping.target_hint.to_lowercase().contains(&folded))
    });
    (!terms.is_empty()).then_some(BusinessKnowledgeMatchType::Partial)
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
