use std::{cell::OnceCell, collections::BTreeMap};

use crate::domain::{
    CodeRepositoryCrossEdge, CodeRepositorySetMemberStatus, CodeRepositorySetQueryHit,
    CodeRetrievalHit,
};

pub(super) struct OverlayEvidenceIndex<'a> {
    edges: &'a [CodeRepositoryCrossEdge],
    import_origins: OnceCell<ImportOriginIndex>,
    target_files: BTreeMap<(String, String), Vec<usize>>,
    target_symbols: BTreeMap<(String, String), Vec<usize>>,
}

type ImportOriginIndex = BTreeMap<(String, String, u32, u32), Vec<usize>>;

impl<'a> OverlayEvidenceIndex<'a> {
    pub(super) fn new(edges: &'a [CodeRepositoryCrossEdge]) -> Self {
        let mut index = Self {
            edges,
            import_origins: OnceCell::new(),
            target_files: BTreeMap::new(),
            target_symbols: BTreeMap::new(),
        };
        for (position, edge) in edges.iter().enumerate() {
            let Some(target_scope) = edge.to_source_scope.clone() else {
                continue;
            };
            let Some(target_record_id) = edge.to_record_id.clone() else {
                continue;
            };
            match edge.to_record_kind.as_str() {
                "code_file" => index
                    .target_files
                    .entry((target_scope, target_record_id))
                    .or_default()
                    .push(position),
                "code_symbol_snapshot" => index
                    .target_symbols
                    .entry((target_scope, target_record_id))
                    .or_default()
                    .push(position),
                _ => {}
            }
        }

        index
    }

    pub(super) fn evidence_for_hit(&self, hit: &CodeRetrievalHit) -> Vec<CodeRepositoryCrossEdge> {
        let mut matches = BTreeMap::<usize, ()>::new();
        if hit.edge_kind.as_deref() == Some("import") {
            let import_origins = self
                .import_origins
                .get_or_init(|| self.build_import_origin_index());
            self.collect(
                import_origins.get(&(
                    hit.scope_id.clone(),
                    hit.path.clone(),
                    hit.line_range.start,
                    hit.line_range.end,
                )),
                &mut matches,
            );
        }
        if let Some(symbol_id) = &hit.symbol_snapshot_id {
            self.collect(
                self.target_symbols
                    .get(&(hit.scope_id.clone(), symbol_id.clone())),
                &mut matches,
            );
        }
        if let Some(file_id) = &hit.file_id {
            self.collect(
                self.target_files
                    .get(&(hit.scope_id.clone(), file_id.clone())),
                &mut matches,
            );
        }

        matches
            .into_keys()
            .take(5)
            .map(|position| self.edges[position].clone())
            .collect()
    }

    fn build_import_origin_index(&self) -> ImportOriginIndex {
        let mut import_origins = BTreeMap::new();
        for (position, edge) in self.edges.iter().enumerate() {
            if edge.from_record_kind != "module_reference" {
                continue;
            }
            if let Some((path, line_start, line_end)) = evidence_origin(&edge.evidence_json) {
                import_origins
                    .entry((edge.from_source_scope.clone(), path, line_start, line_end))
                    .or_insert_with(Vec::new)
                    .push(position);
            }
        }

        import_origins
    }

    fn collect(&self, edges: Option<&Vec<usize>>, matches: &mut BTreeMap<usize, ()>) {
        let Some(edges) = edges else {
            return;
        };
        for position in edges {
            matches.insert(*position, ());
        }
    }
}

pub(super) fn per_member_candidate_limit(limit: usize) -> usize {
    std::cmp::min(
        50,
        std::cmp::max(limit.saturating_mul(3), limit.saturating_add(5)),
    )
}

pub(super) fn repository_set_score(
    hit: &CodeRetrievalHit,
    member: &CodeRepositorySetMemberStatus,
    overlay_evidence: &[CodeRepositoryCrossEdge],
) -> f64 {
    let priority_bonus = f64::from(member.member.priority) * 0.01;
    let freshness_penalty = if hit.stale || member.stale { 0.5 } else { 0.0 };
    let edge_bonus = overlay_evidence
        .iter()
        .map(|edge| f64::from(edge.confidence_basis_points) / 10_000.0)
        .fold(0.0, f64::max);

    hit.score + priority_bonus + edge_bonus - freshness_penalty
}

pub(super) fn dedupe_sort_truncate(
    results: &mut Vec<CodeRepositorySetQueryHit>,
    limit: usize,
) -> bool {
    let mut best =
        BTreeMap::<(String, String, String, u32, u32, String), CodeRepositorySetQueryHit>::new();
    for result in results.drain(..) {
        let key = (
            result.hit.repository_id.clone(),
            result.hit.scope_id.clone(),
            result.hit.path.clone(),
            result.hit.line_range.start,
            result.hit.line_range.end,
            result.hit.excerpt.clone(),
        );
        match best.get(&key) {
            Some(existing) if existing.score >= result.score => {}
            _ => {
                best.insert(key, result);
            }
        }
    }
    results.extend(best.into_values());
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| {
                left.member
                    .repository_alias
                    .cmp(&right.member.repository_alias)
            })
            .then_with(|| left.hit.path.cmp(&right.hit.path))
            .then_with(|| left.hit.line_range.start.cmp(&right.hit.line_range.start))
    });
    let truncated = results.len() > limit;
    results.truncate(limit);
    truncated
}

fn evidence_origin(evidence_json: &str) -> Option<(String, u32, u32)> {
    serde_json::from_str::<serde_json::Value>(evidence_json)
        .ok()
        .and_then(|value| {
            let path = value
                .get("from_path")
                .and_then(|path| path.as_str())
                .map(str::to_owned)?;
            let line_start = value
                .get("from_line_start")
                .and_then(|line| line.as_u64())
                .and_then(|line| u32::try_from(line).ok())?;
            let line_end = value
                .get("from_line_end")
                .and_then(|line| line.as_u64())
                .and_then(|line| u32::try_from(line).ok())?;

            Some((path, line_start, line_end))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySetMember, CodeRetrievalLayer, RepositoryCodeRange};

    #[test]
    fn overlay_index_attaches_target_and_import_origin_evidence_in_edge_order() {
        let inbound = edge(
            "edge-in",
            "scope-service",
            Some("scope-app"),
            r#"{"from_path":"src/service.rs"}"#,
            9_000,
        );
        let outbound = edge(
            "edge-out",
            "scope-app",
            Some("scope-service"),
            r#"{"from_path":"src/client.rs","from_line_start":1,"from_line_end":1}"#,
            6_000,
        );
        let unrelated = edge(
            "edge-other",
            "scope-other",
            Some("scope-service"),
            r#"{"from_path":"src/other.rs","from_line_start":1,"from_line_end":1}"#,
            10_000,
        );
        let mut wrong_target = edge(
            "edge-wrong",
            "scope-service",
            Some("scope-app"),
            "{}",
            8_000,
        );
        wrong_target.to_record_id = Some("symbol-other".to_owned());
        let edges = vec![inbound.clone(), outbound.clone(), unrelated, wrong_target];
        let index = OverlayEvidenceIndex::new(&edges);

        let evidence =
            index.evidence_for_hit(&hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false));
        assert_eq!(evidence, vec![inbound]);

        let mut import_hit = hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false);
        import_hit.symbol_snapshot_id = None;
        import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
        import_hit.edge_kind = Some("import".to_owned());
        assert_eq!(index.evidence_for_hit(&import_hit), vec![outbound]);
    }

    #[test]
    fn overlay_index_dedupes_and_caps_multi_key_evidence() {
        let mut edges = Vec::new();
        for index in 0..8 {
            edges.push(edge(
                &format!("edge-{index}"),
                "scope-service",
                Some("scope-app"),
                r#"{"from_path":"src/service.rs"}"#,
                5_000,
            ));
        }
        let mut file_edge = edge(
            "edge-file",
            "scope-service",
            Some("scope-app"),
            r#"{"from_path":"src/service.rs"}"#,
            4_000,
        );
        file_edge.to_record_kind = "code_file".to_owned();
        file_edge.to_record_id = Some("file-1".to_owned());
        edges.insert(2, file_edge.clone());
        let index = OverlayEvidenceIndex::new(&edges);

        let evidence =
            index.evidence_for_hit(&hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false));

        assert_eq!(evidence.len(), 5);
        assert_eq!(evidence[2], file_edge);
    }

    #[test]
    fn ranking_helpers_keep_existing_merge_policy() {
        let member = member_status("app", "scope-app", 7);
        let base_hit = hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, false);
        let evidence = vec![edge(
            "edge-in",
            "scope-service",
            Some("scope-app"),
            r#"{"from_path":"src/service.rs"}"#,
            9_000,
        )];
        assert!(repository_set_score(&base_hit, &member, &evidence) > base_hit.score);
        assert!(
            repository_set_score(
                &hit("repo-a", "scope-app", "src/client.rs", 1, 0.75, true),
                &member,
                &[]
            ) < base_hit.score
        );

        let mut results = vec![
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 1, 0.50, false),
                overlay_evidence: Vec::new(),
                score: 0.50,
            },
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 1, 0.90, false),
                overlay_evidence: evidence,
                score: 0.90,
            },
            CodeRepositorySetQueryHit {
                member: member.member.clone(),
                hit: hit("repo-a", "scope-app", "src/client.rs", 2, 0.80, false),
                overlay_evidence: Vec::new(),
                score: 0.80,
            },
        ];
        assert!(dedupe_sort_truncate(&mut results, 1));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 0.90);

        assert_eq!(per_member_candidate_limit(1), 6);
        assert_eq!(per_member_candidate_limit(20), 50);
        assert!(evidence_origin("not-json").is_none());
        assert!(evidence_origin("{}").is_none());
    }

    fn member_status(
        repository_alias: &str,
        source_scope: &str,
        priority: i32,
    ) -> CodeRepositorySetMemberStatus {
        CodeRepositorySetMemberStatus {
            member: CodeRepositorySetMember {
                set_id: "set-workspace".to_owned(),
                repository_id: format!("repo-{repository_alias}"),
                repository_alias: repository_alias.to_owned(),
                ref_selector: "HEAD".to_owned(),
                resolved_commit_sha: format!("commit-{source_scope}"),
                source_scope: source_scope.to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
                priority,
            },
            tree_hash: format!("tree-{source_scope}"),
            freshness_state: "fresh".to_owned(),
            stale: false,
            indexed_file_count: 1,
            symbol_count: 1,
            reference_count: 0,
            chunk_count: 1,
            degraded_reason: None,
        }
    }

    fn hit(
        repository_id: &str,
        scope_id: &str,
        path: &str,
        line: u32,
        score: f64,
        stale: bool,
    ) -> CodeRetrievalHit {
        CodeRetrievalHit {
            repository_id: repository_id.to_owned(),
            scope_id: scope_id.to_owned(),
            resolved_commit_sha: format!("commit-{scope_id}"),
            tree_hash: format!("tree-{scope_id}"),
            path: path.to_owned(),
            language_id: "rust".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 10 },
            line_range: RepositoryCodeRange {
                start: line,
                end: line,
            },
            symbol_snapshot_id: Some(format!("symbol-{line}")),
            canonical_symbol_id: None,
            file_id: Some("file-1".to_owned()),
            retrieval_layers: vec![CodeRetrievalLayer::Symbol],
            index_versions: vec!["code:1".to_owned()],
            stale,
            degraded_reason: None,
            edge_kind: None,
            edge_resolution_state: None,
            edge_target_hint: None,
            edge_confidence_basis_points: None,
            edge_confidence_tier: None,
            score,
            excerpt: format!("excerpt {line}"),
        }
    }

    fn edge(
        edge_id: &str,
        from_scope: &str,
        to_scope: Option<&str>,
        evidence_json: &str,
        confidence: u16,
    ) -> CodeRepositoryCrossEdge {
        CodeRepositoryCrossEdge {
            edge_id: edge_id.to_owned(),
            set_id: "set-workspace".to_owned(),
            from_source_scope: from_scope.to_owned(),
            from_repository_id: "repo-from".to_owned(),
            from_record_kind: "module_reference".to_owned(),
            from_record_id: "import-1".to_owned(),
            to_source_scope: to_scope.map(str::to_owned),
            to_repository_id: to_scope.map(|_| "repo-to".to_owned()),
            to_record_kind: "code_symbol_snapshot".to_owned(),
            to_record_id: to_scope.map(|_| "symbol-1".to_owned()),
            edge_kind: "imports".to_owned(),
            resolution_state: "resolved".to_owned(),
            confidence_basis_points: confidence,
            confidence_tier: "explicit".to_owned(),
            evidence_json: evidence_json.to_owned(),
            created_at_ms: 10,
        }
    }
}
