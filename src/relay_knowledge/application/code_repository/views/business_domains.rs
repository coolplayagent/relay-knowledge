use std::collections::BTreeMap;

use crate::domain::CodebaseViewSnapshot;

use super::{
    builder::{SectionRefs, ViewBuilder},
    rules::{domain_confidence, domain_token, path_domain, route_domain},
};

pub(super) fn derive_business_domains(builder: &mut ViewBuilder, snapshot: &CodebaseViewSnapshot) {
    let mut domains = BTreeMap::<String, Vec<String>>::new();
    for route in &snapshot.routes {
        if let Some(domain) = route_domain(&route.url) {
            let evidence_id = builder.evidence(
                "route",
                &route.path,
                Some(route.handler_name.clone()),
                Some(route.line_range.clone()),
                Some(route.http_method.clone()),
                format!("{} {}", route.http_method, route.url),
            );
            domains.entry(domain).or_default().push(evidence_id);
        }
    }
    for flag in &snapshot.feature_flags {
        if let Some(domain) = domain_token(&flag.name) {
            let evidence_id = builder.evidence(
                "feature_flag",
                &flag.path,
                Some(flag.name.clone()),
                Some(flag.line_range.clone()),
                Some(flag.edge_kind.clone()),
                format!("feature flag {}", flag.source_key),
            );
            domains.entry(domain).or_default().push(evidence_id);
        }
    }
    for file in &snapshot.files {
        if let Some(domain) = path_domain(&file.path) {
            let evidence_id = builder.evidence(
                "path",
                &file.path,
                None,
                None,
                None,
                "domain-like path segment",
            );
            domains.entry(domain).or_default().push(evidence_id);
        }
    }
    let mut ordered = domains.into_iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.0.cmp(&right.0)));
    if ordered.len() > builder.limit {
        builder.mark_node_budget_truncated();
    }
    for (domain, evidence_ids) in ordered.into_iter().take(builder.limit) {
        let node_id = builder.node(
            format!("domain:{domain}"),
            domain.clone(),
            "business_domain",
            None,
            domain_confidence(evidence_ids.len()),
            evidence_ids.first().cloned(),
        );
        builder.section(
            format!("section:domain:{domain}"),
            format!("{domain} domain"),
            format!(
                "{domain} is a candidate business domain from {} route, feature flag, or path signal(s).",
                evidence_ids.len()
            ),
            domain_confidence(evidence_ids.len()),
            SectionRefs {
                node_ids: node_id.into_iter().collect(),
                evidence_ids,
                ..SectionRefs::default()
            },
        );
    }
}
