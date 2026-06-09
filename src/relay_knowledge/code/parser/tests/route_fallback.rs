use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn parser_failure_fallback_records_routes() {
    let content = "app.get('/health', (req, res) => res.end());\n";
    let mut build = route_fallback_build();

    parse_syntax_file(
        &mut build,
        SyntaxFileInput {
            path: "src/routes.ts",
            file_id: "routes-file",
            language: LanguageSpec {
                id: "typescript",
                language: || panic!("parser boom"),
                tags_query: "",
            },
            blob_hash: "hash",
            byte_len: content.len(),
            line_count: 1,
            content,
        },
    )
    .expect("parser failure should be isolated");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Failed);
    assert_eq!(snapshot.routes.len(), 1);
    assert_eq!(snapshot.routes[0].url, "/health");
    assert_eq!(snapshot.routes[0].handler_name, "anonymous");
    assert!(snapshot.routes[0].handler_symbol_snapshot_id.is_none());
}

#[test]
fn query_failure_fallback_records_routes() {
    let content = "app.get('/ready', ready);\n";
    let mut build = route_fallback_build();

    parse_syntax_file(
        &mut build,
        SyntaxFileInput {
            path: "src/routes.ts",
            file_id: "routes-file",
            language: LanguageSpec {
                id: "typescript",
                language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                tags_query: "(",
            },
            blob_hash: "hash",
            byte_len: content.len(),
            line_count: 1,
            content,
        },
    )
    .expect("query failure should be isolated");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Failed);
    assert_eq!(snapshot.routes.len(), 1);
    assert_eq!(snapshot.routes[0].url, "/ready");
}

#[test]
fn text_only_known_language_records_routes() {
    let mut content = "app.get('/status', status);\n".to_owned();
    content.push_str(&"x".repeat(MAX_TEXT_FILE_BYTES));
    let mut build = route_fallback_build();

    parse_indexed_file(&mut build, "src/routes.ts", content.as_bytes())
        .expect("text-only route file should parse");
    let snapshot = build.finish();

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::TextOnly);
    assert_eq!(snapshot.routes.len(), 1);
    assert_eq!(snapshot.routes[0].url, "/status");
}

fn route_fallback_build() -> SnapshotBuild {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    )
}
