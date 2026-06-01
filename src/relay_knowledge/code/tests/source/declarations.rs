use std::fs;

use super::{
    resolve_repository_ref, resolve_repository_ref_with_filters, source_declarations_for_identity,
    test_fixtures::{TempGitRepo, TempSourceDir},
};

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
        &[],
        &[],
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
        &[],
        &[],
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
        &[],
        &[],
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
            &[],
            &[],
            "api_fn()",
        )
        .expect("invalid identity should not fail")
        .is_empty()
    );
}

#[test]
fn source_declaration_fallback_verifies_language_filtered_filesystem_commit() {
    let source = TempSourceDir::create("source-declaration-filesystem-language");
    source.write("src/lib.rs", "pub fn filesystem_decl_target() {}\n");
    source.write(
        "src/index.ts",
        "export function filesystemDeclarationTarget() {}\n",
    );
    let language_filters = vec!["rust".to_owned()];
    let registration = source.registration();
    let commit = resolve_repository_ref_with_filters(&source.path, "HEAD", &[], &language_filters)
        .expect("language-filtered filesystem commit should resolve");

    let matches = source_declarations_for_identity(
        &registration,
        &commit,
        vec!["src/lib.rs".to_owned()],
        &[],
        &language_filters,
        "filesystem_decl_target",
    )
    .expect("source declaration fallback should verify language-filtered filesystem commit");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].path, "src/lib.rs");
}

#[test]
fn source_declaration_fallback_requires_git_objects_for_git_commits() {
    let repo = TempGitRepo::create("source-declaration-git-authority");
    repo.write("src/lib.rs", "pub fn committed_target() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);
    fs::remove_dir_all(repo.path.join(".git")).expect("git metadata should be removable");
    repo.write("src/lib.rs", "pub fn edited_live_target() {}\n");

    let matches = source_declarations_for_identity(
        &repo.registration(),
        &commit,
        vec!["src/lib.rs".to_owned()],
        &[],
        &[],
        "edited_live_target",
    )
    .expect("missing git objects should not fall back to live filesystem bytes");

    assert!(matches.is_empty());
}

#[test]
fn source_declaration_fallback_verifies_stored_scope_before_narrow_query_scope() {
    let source = TempSourceDir::create("source-declaration-filesystem-broad");
    source.write("src/lib.rs", "pub fn declaration_target() {}\n");
    source.write("src/other.rs", "pub fn other_target() {}\n");
    let commit =
        resolve_repository_ref(&source.path, "HEAD").expect("filesystem ref should resolve");

    let matches = source_declarations_for_identity(
        &source.registration(),
        &commit,
        vec!["src/lib.rs".to_owned()],
        &["src/lib.rs".to_owned()],
        &[],
        "declaration_target",
    )
    .expect("source declaration fallback should verify the stored broad scope");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].path, "src/lib.rs");
}
