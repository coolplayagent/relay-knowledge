use std::path::{Path, PathBuf};

use crate::{
    application::knowledge::map::KnowledgeMapService,
    project::{AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_RELATIVE_PATH},
};

pub(crate) fn knowledge_map_service() -> Result<KnowledgeMapService, String> {
    let current = std::env::current_dir()
        .map_err(|error| format!("failed to resolve current directory: {error}"))?;
    repository_root_from(&current)
        .map(KnowledgeMapService::new)
        .ok_or_else(|| format!("failed to find repository root for {KNOWLEDGE_MAP_RELATIVE_PATH}"))
}

fn repository_root_from(current: &Path) -> Option<PathBuf> {
    let mut agents_root = None;
    for path in current.ancestors() {
        if path.join(".git").exists() || path.join(AGENT_CONTRACT_DIR_NAME).exists() {
            return Some(path.to_path_buf());
        }
        if path.join("AGENTS.md").exists() {
            agents_root = Some(path.to_path_buf());
        }
    }
    agents_root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_root_search_walks_up_from_subdirectory() {
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-root-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should work")
                .as_nanos()
        ));
        let nested = root.join("src").join("module");
        std::fs::create_dir_all(&nested).expect("nested dir should create");
        std::fs::write(
            root.join("AGENTS.md"),
            "Knowledge map: .knowledge/knowledge-map.yaml",
        )
        .expect("agents should write");

        let discovered = repository_root_from(&nested).expect("root should be found");

        assert_eq!(discovered, root);
        let _ = std::fs::remove_dir_all(discovered);
    }

    #[test]
    fn repository_root_search_ignores_scoped_agents_when_git_root_exists() {
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-root-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should work")
                .as_nanos()
        ));
        let nested = root.join("src").join("module");
        std::fs::create_dir_all(root.join(".git")).expect("git marker should create");
        std::fs::create_dir_all(&nested).expect("nested dir should create");
        std::fs::write(nested.join("AGENTS.md"), "Scoped instructions.")
            .expect("scoped agents should write");

        let discovered = repository_root_from(&nested).expect("root should be found");

        assert_eq!(discovered, root);
        let _ = std::fs::remove_dir_all(discovered);
    }
}
