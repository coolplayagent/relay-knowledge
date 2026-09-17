//! Canonical source-language constraints carried by durable scope identities.
//!
//! Each group is an OR of language predicates; groups are ANDed. Internal
//! encoding keeps existing snapshot/checkpoint records readable. Source admission
//! evaluates every group. Queries bound to that admitted scope use the atom union
//! as their evidence mask, preserving manifest and document language semantics.
use std::collections::BTreeSet;

const PREFIX: &str = "__relay_language_groups__:";

pub(crate) fn validate_code_language_filters(filters: &[String]) -> Result<(), super::DomainError> {
    let invalid = || {
        super::DomainError::invalid(
            "language_filter",
            "malformed or over-budget source language constraints",
        )
    };
    if filters.len() > 256 || filters.iter().map(String::len).sum::<usize>() > 65_536 {
        return Err(invalid());
    }
    if filters.iter().any(|value| value.starts_with(PREFIX)) {
        let [encoded] = filters else {
            return Err(invalid());
        };
        let Some(groups) = encoded.strip_prefix(PREFIX) else {
            return Err(invalid());
        };
        let mut group_count = 0;
        let mut term_count = 0;
        for group in groups.split(';') {
            group_count += 1;
            if group_count > 32 {
                return Err(invalid());
            }
            for term in group.split(',') {
                term_count += 1;
                if term_count > 1_024
                    || term.is_empty()
                    || term.starts_with("__relay_")
                    || !term
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                {
                    return Err(invalid());
                }
            }
        }
        if group_count < 2 {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn code_language_filter_groups(filters: &[String]) -> Vec<Vec<&str>> {
    if validate_code_language_filters(filters).is_err() {
        return vec![vec!["__relay_invalid_language_constraints__"]];
    }
    if let [encoded] = filters
        && let Some(groups) = encoded.strip_prefix(PREFIX)
    {
        return groups
            .split(';')
            .map(|group| group.split(',').collect())
            .collect();
    }
    if filters.is_empty() {
        Vec::new()
    } else {
        vec![filters.iter().map(String::as_str).collect()]
    }
}

pub(crate) fn code_scope_language_filters(left: &[String], right: &[String]) -> Vec<String> {
    let groups = code_language_filter_groups(left)
        .into_iter()
        .chain(code_language_filter_groups(right))
        .map(|group| group.into_iter().collect::<BTreeSet<_>>())
        .collect::<BTreeSet<_>>();
    let groups = groups
        .iter()
        .filter(|group| {
            !groups
                .iter()
                .any(|other| other.len() < group.len() && other.is_subset(group))
        })
        .map(|group| group.iter().copied().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    match groups.as_slice() {
        [] => Vec::new(),
        [group] => group.iter().map(|value| (*value).to_owned()).collect(),
        _ => vec![format!(
            "{PREFIX}{}",
            groups
                .iter()
                .map(|group| group.join(","))
                .collect::<Vec<_>>()
                .join(";")
        )],
    }
}

/// Coverage requires both source-predicate implication and evidence languages.
/// Shared manifests emit facts in the admitted language mask, so equal paths
/// alone cannot prove that a narrower scope contains all requested facts.
pub(crate) fn code_language_scope_covers(stored: &[String], requested: &[String]) -> bool {
    if validate_code_language_filters(stored).is_err()
        || validate_code_language_filters(requested).is_err()
    {
        return false;
    }
    let stored = code_language_filter_groups(stored);
    let requested = code_language_filter_groups(requested);
    let evidence_covered = stored.is_empty()
        || requested
            .iter()
            .flatten()
            .all(|value| stored.iter().any(|group| group.contains(value)));
    evidence_covered
        && stored.iter().all(|outer| {
            requested
                .iter()
                .any(|inner| inner.iter().all(|value| outer.contains(value)))
        })
}

/// Evidence mask for an already admitted, exact source scope; never source admission.
pub(crate) fn code_language_filter_atoms(filters: &[String]) -> Vec<String> {
    code_language_filter_groups(filters)
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_scope_never_covers_a_different_language_or_unfiltered_registration() {
        let broad = vec!["bash".into(), "python".into()];
        assert!(code_language_scope_covers(&broad, &["bash".into()]));
        assert!(!code_language_scope_covers(&["bash".into()], &broad));
        assert!(!code_language_scope_covers(
            &["bash".into()],
            &["python".into()]
        ));
        assert!(!code_language_scope_covers(&["bash".into()], &[]));
        assert!(code_language_scope_covers(&[], &broad));
    }

    #[test]
    fn manifest_path_overlap_does_not_imply_evidence_language_coverage() {
        let java = vec!["java".into()];
        let kotlin = vec!["kotlin".into()];
        let joint = code_scope_language_filters(&java, &kotlin);
        assert!(!code_language_scope_covers(&java, &joint));
        assert!(!code_language_scope_covers(&kotlin, &joint));
        assert!(code_language_scope_covers(&joint, &joint));
        assert!(code_language_scope_covers(
            &["java".into(), "kotlin".into()],
            &joint
        ));
        assert!(!code_language_scope_covers(&joint, &java));
    }

    #[test]
    fn malformed_and_excessive_constraint_encodings_fail_closed() {
        for encoded in [
            format!("{PREFIX}rust;"),
            format!("{PREFIX}rust"),
            format!("{PREFIX}rust;__relay_other__"),
            format!("{PREFIX}{}", vec!["rust"; 33].join(";")),
            format!("{PREFIX}rust;{}", vec!["toml"; 1_024].join(",")),
            "x".repeat(65_537),
        ] {
            assert!(validate_code_language_filters(std::slice::from_ref(&encoded)).is_err());
            assert_eq!(
                code_language_filter_atoms(&[encoded]),
                ["__relay_invalid_language_constraints__"]
            );
        }
    }

    #[test]
    fn public_selectors_reject_internal_constraints_after_json_deserialization() {
        let filters = code_scope_language_filters(&["rust".into()], &["toml".into()]);
        assert!(
            super::super::CodeRepositorySelector::new("repo", "HEAD", vec![], filters.clone())
                .is_err()
        );
        let json = serde_json::json!({"repository":"repo", "ref_selector":"HEAD", "path_filters":[], "language_filters":filters});
        assert!(serde_json::from_value::<super::super::CodeRepositorySelector>(json).is_err());
    }

    #[test]
    fn canonical_groups_preserve_conjunction_without_product_expansion() {
        let a = vec!["bash".into(), "python".into()];
        assert_eq!(code_scope_language_filters(&a, &["bash".into()]), ["bash"]);
        let groups = code_scope_language_filters(&["rust".into()], &["toml".into()]);
        assert_eq!(
            code_language_filter_groups(&groups),
            [vec!["rust"], vec!["toml"]]
        );
        assert_eq!(code_language_filter_atoms(&groups), ["rust", "toml"]);
        assert_eq!(
            code_scope_language_filters(&groups, &["rust".into()]),
            groups
        );
        assert_ne!(
            code_scope_language_filters(&a, &["bash".into()]),
            code_scope_language_filters(&a, &["python".into()])
        );
    }
}
