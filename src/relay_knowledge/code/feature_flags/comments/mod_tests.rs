//! Unit contract for feature-flag comment and string shielding.

use super::*;

fn input(path: &'static str, language_id: &'static str) -> FeatureFlagFileInput<'static> {
    FeatureFlagFileInput {
        repository_id: "repo",
        source_scope: "scope",
        file_id: "file",
        path,
        language_id,
        content: "",
        config_facts: &[],
    }
}

#[test]
fn scan_line_removes_block_and_trailing_comments() {
    let mut state = CommentState::default();
    let scan_line = state
        .scan_line(
            "let live = true; /* hidden */ use(live); // ignored",
            &input("src/lib.rs", "rust"),
        )
        .expect("live source should remain scannable");

    assert!(scan_line.contains("let live = true"));
    assert!(scan_line.contains("use(live)"));
    assert!(!scan_line.contains("hidden"));
    assert!(!scan_line.contains("ignored"));
}

#[test]
fn nested_block_state_survives_line_boundaries() {
    let mut state = CommentState::default();
    let input = input("src/lib.rs", "rust");

    assert_eq!(state.scan_line("/* outer", &input), None);
    assert_eq!(state.scan_line("/* inner */ still outer", &input), None);
    assert_eq!(
        state.scan_line("*/ live_call()", &input),
        Some(" live_call()".to_owned())
    );
}

#[test]
fn config_paths_treat_hash_lines_as_comments() {
    let mut state = CommentState::default();
    let input = input("config/flags.toml", "toml");

    assert_eq!(state.scan_line("# disabled = true", &input), None);
    assert_eq!(
        state.scan_line("enabled = true # trailing", &input),
        Some("enabled = true ".to_owned())
    );
}
