use super::*;

#[test]
fn history_paths_keep_runtime_state_under_repository_git_data() {
    let paths = HistoryPaths::new(std::path::Path::new("/tmp/repository"));

    assert_eq!(
        paths.root,
        std::path::Path::new("/tmp/repository/.git/relay-knowledge-self-iteration")
    );
    assert_eq!(paths.memory_index, paths.memory.join("index.jsonl"));
    assert_eq!(paths.score_svg, paths.root.join("score-v2.svg"));
}
