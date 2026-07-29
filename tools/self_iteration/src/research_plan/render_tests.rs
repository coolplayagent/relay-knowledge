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
