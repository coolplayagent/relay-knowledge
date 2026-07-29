#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvaluationCategory {
    Foundational,
    Competitive,
    SemanticVector,
    FileFixtures,
    RepositorySets,
    AgentWorkflows,
    ResearchJudge,
    Performance,
}

impl EvaluationCategory {
    const ALL: [Self; 8] = [
        Self::Foundational,
        Self::Competitive,
        Self::SemanticVector,
        Self::FileFixtures,
        Self::RepositorySets,
        Self::AgentWorkflows,
        Self::ResearchJudge,
        Self::Performance,
    ];

    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "foundational" | "foundational_capability" => Ok(Self::Foundational),
            "competitive" | "competitive_capability" => Ok(Self::Competitive),
            "semantic_vector" | "semantic-vector" | "semantic" | "vector" => {
                Ok(Self::SemanticVector)
            }
            "file_fixtures" | "file-fixtures" | "files" => Ok(Self::FileFixtures),
            "repository_sets" | "repository-sets" | "repo_sets" | "repo-sets" => {
                Ok(Self::RepositorySets)
            }
            "agent_workflows" | "agent-workflows" | "agent" | "coding_agent" | "coding-agent" => {
                Ok(Self::AgentWorkflows)
            }
            "research_judge" | "research-judge" | "judge" => Ok(Self::ResearchJudge),
            "performance" => Ok(Self::Performance),
            other => Err(format!("invalid evaluation category: {other}")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Foundational => "foundational",
            Self::Competitive => "competitive",
            Self::SemanticVector => "semantic_vector",
            Self::FileFixtures => "file_fixtures",
            Self::RepositorySets => "repository_sets",
            Self::AgentWorkflows => "agent_workflows",
            Self::ResearchJudge => "research_judge",
            Self::Performance => "performance",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorySet {
    categories: BTreeSet<EvaluationCategory>,
}

impl CategorySet {
    pub fn parse(value: &str) -> Result<Self, String> {
        let mut categories = BTreeSet::new();
        for item in value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            if item.eq_ignore_ascii_case("all") {
                categories.extend(EvaluationCategory::ALL);
            } else {
                categories.insert(EvaluationCategory::parse(item)?);
            }
        }
        if categories.is_empty() {
            return Err("--categories must include at least one category".to_owned());
        }
        Ok(Self { categories })
    }

    pub(super) fn all() -> Self {
        Self {
            categories: EvaluationCategory::ALL.into_iter().collect(),
        }
    }

    pub fn contains(&self, category: EvaluationCategory) -> bool {
        self.categories.contains(&category)
    }

    pub fn single(category: EvaluationCategory) -> Self {
        let mut categories = BTreeSet::new();
        categories.insert(category);
        Self { categories }
    }

    pub fn labels(&self) -> Vec<&'static str> {
        EvaluationCategory::ALL
            .into_iter()
            .filter(|category| self.contains(*category))
            .map(EvaluationCategory::label)
            .collect()
    }

    pub fn focus_key(&self) -> String {
        self.labels().join(",")
    }

    pub(super) fn remove_all(&mut self, excluded: &Self) {
        for category in &excluded.categories {
            self.categories.remove(category);
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.categories.is_empty()
    }
}

#[cfg(test)]
#[path = "categories_tests.rs"]
mod categories_tests;
use std::collections::BTreeSet;
