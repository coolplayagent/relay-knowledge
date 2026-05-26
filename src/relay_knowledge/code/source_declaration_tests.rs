use super::{source_declarations_for_identity, test_fixtures::TempGitRepo};

#[test]
fn source_declaration_fallback_reads_exact_committed_typedef() {
    let repo = TempGitRepo::create("source-declaration-typedef");
    repo.write(
        "include/driver_ops.h",
        "struct rk_device;\n\
         typedef int (*rk_read_fn)(struct rk_device *dev);\n\
         struct rk_driver_ops {\n\
             rk_read_fn read;\n\
         };\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    repo.write("include/driver_ops.h", "typedef int (*rk_read_fn)(void);\n");

    let matches = source_declarations_for_identity(
        &repo.registration(),
        &commit,
        vec!["include/driver_ops.h".to_owned()],
        "rk_read_fn",
    )
    .expect("source declaration fallback should read committed blob");

    assert_eq!(matches.len(), 1);
    assert_eq!(
        matches[0].excerpt,
        "typedef int (*rk_read_fn)(struct rk_device *dev);"
    );
    assert_eq!(matches[0].line_range.start, 2);
}

#[test]
fn source_declaration_fallback_skips_call_like_uses_before_declaration() {
    let repo = TempGitRepo::create("source-declaration-calls");
    repo.write(
        "src/callback.c",
        "void register_all(void) { register_callback(api_fn()); }\n\
         int api_fn(void);\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);

    let matches = source_declarations_for_identity(
        &repo.registration(),
        &commit,
        vec!["src/callback.c".to_owned()],
        "api_fn",
    )
    .expect("source declaration fallback should skip call-like use");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].excerpt, "int api_fn(void);");
    assert_eq!(matches[0].line_range.start, 2);
}

#[test]
fn source_declaration_fallback_ignores_unsafe_paths_and_non_identities() {
    let repo = TempGitRepo::create("source-declaration-paths");
    repo.write("include/api.h", "typedef int api_fn(void);\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);

    let matches = source_declarations_for_identity(
        &repo.registration(),
        &commit,
        vec![
            "../include/api.h".to_owned(),
            "/include/api.h".to_owned(),
            "include/api.h".to_owned(),
        ],
        "api_fn",
    )
    .expect("safe path should still be read");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].path, "include/api.h");
    assert!(
        source_declarations_for_identity(
            &repo.registration(),
            &commit,
            vec!["include/api.h".to_owned()],
            "api_fn()",
        )
        .expect("invalid identity should not fail")
        .is_empty()
    );
}
