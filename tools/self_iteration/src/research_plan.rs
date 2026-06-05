pub struct ResearchPlanInput<'a> {
    pub topic: &'a str,
    pub slug: &'a str,
    pub date: &'a str,
}

pub fn render(input: ResearchPlanInput<'_>) -> String {
    let topic = input.topic.trim();
    let slug = input.slug.trim();
    let date = input.date.trim();
    format!(
        r#"# Research Self-Iteration Plan: {topic}

> Date: {date}
> Slug: {slug}
> Purpose: reusable research loop for source-backed relay-knowledge roadmap work.

## 1. Reference Action Summary

Use the 2026 graph-database and CodeGraph research pass as the baseline method:

1. Inspect the clean worktree, branch, remotes, existing research docs, verification archives, and issue state before trusting previous context.
2. Gather real primary or traceable sources across arXiv/papers, X.com trend captures, Reddit discussions, open-source repositories, official docs, and systems-engineering references.
3. Separate evidence classes: papers and official docs prove technical claims, open-source code proves feasibility, social sources reveal demand and skepticism, and internal benchmarks prove relay-knowledge impact.
4. Update bilingual research documentation with a source ledger, comparison matrix, product implications, risk boundaries, and implementation principles.
5. File one GitHub issue per competitive feature, with source-backed rationale, scope boundaries, and acceptance signals.
6. Add a dated verification/archive record that preserves sources, research artifacts, issues, validation commands, and publication state.
7. Run appropriate checks, commit the documentation/tooling change, push to remote main, and verify local/remote HEAD equality.

## 2. Source Ledger Checklist

- [ ] arXiv or peer-reviewed papers: record title, URL, date/version, claimed method, and which claim the paper actually supports.
- [ ] Official docs or product docs: record protocol/API/product facts and avoid replacing them with commentary.
- [ ] X.com: record direct status URLs when available and a traceable capture page when logged-out access is script-only; use only for adoption heat and product language.
- [ ] Reddit or community threads: record real user pain, trust concerns, and workflow expectations; do not treat anecdotes as performance proof.
- [ ] Open-source repositories: record README claims, implementation surface, release/activity signal, and whether code is sufficient to trust the claim.
- [ ] Internal relay-knowledge evidence: record affected docs, tests, benchmarks, issues, and architecture constraints.

## 3. Research Synthesis Template

For each source-backed theme, fill this row before writing product conclusions:

| Theme | Sources | Verified signal | Risk or missing proof | relay-knowledge implication | Candidate issue |
| --- | --- | --- | --- | --- | --- |
| Example | arXiv + repo + discussion | What is directly evidenced | What remains unproven | Architecture/product principle | Issue title or N/A |

## 4. Competitive Issue Extraction

Create separate issues only when a feature is independently valuable and testable.
Each issue body should include:

- Source-backed signal: links and the exact evidence class each source provides.
- Competitive feature: one product capability, not a bundled roadmap.
- Acceptance boundaries: freshness, source scope, authorization, bounded queues, versioning, tests, and anti-fixture constraints.
- Non-goals: claims that social sources or unverified demos do not prove.

## 5. Documentation and Archive Outputs

- [ ] Update or create the main research note under `docs/zh/04-research/` and `docs/en/04-research/`.
- [ ] Update research indexes when the new or changed note should be discoverable.
- [ ] Add a dated archive under `docs/zh/06-verification/` and `docs/en/06-verification/`.
- [ ] Link the archive from the bilingual bookshelf indexes.
- [ ] Keep every tracked source, docs, script, workflow, and test file below 1000 lines.

## 6. Validation Gates

Recommended local checks for research/documentation/tooling iterations:

```bash
git diff --check
cargo fmt --all -- --check
cargo test --manifest-path tools/self_iteration/Cargo.toml
```

Run broader gates when Rust product code, CLI behavior, release docs, workflows, or storage/indexing behavior changes:

```bash
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

## 7. Completion Evidence

- [ ] Research note contains real URLs and distinguishes source credibility tiers.
- [ ] Archive records source ledger, artifacts, issues, validation, and commit/push state.
- [ ] GitHub issues exist and are queryable.
- [ ] Local worktree is clean.
- [ ] Remote `main` HEAD matches local HEAD after push.
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_reusable_research_method_sections() {
        let plan = render(ResearchPlanInput {
            topic: "graph database research",
            slug: "graph-database-research",
            date: "2026-06-05",
        });

        assert!(plan.contains("Research Self-Iteration Plan: graph database research"));
        assert!(plan.contains("Source Ledger Checklist"));
        assert!(plan.contains("Competitive Issue Extraction"));
        assert!(plan.contains("Documentation and Archive Outputs"));
        assert!(plan.contains("Remote `main` HEAD matches local HEAD"));
    }
}
