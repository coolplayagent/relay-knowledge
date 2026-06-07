use super::*;
use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:chunk-ranking:commit:tree";

#[test]
fn declaration_bonus_boosts_type_relationship_surfaces() {
    let terms = query_terms("BloomFilterPolicy inherits FilterPolicy KeyMayMatch override");

    assert_eq!(
        declaration_chunk_bonus(
            &terms,
            "class BloomFilterPolicy : public FilterPolicy {\n public:\n  void CreateFilter(const Slice* keys, int n, std::string* dst) const override;\n  bool KeyMayMatch(const Slice& key, const Slice& filter) const override;\n};",
        ),
        5.75
    );
}

#[test]
fn declaration_bonus_accepts_exported_class_relationship_surfaces() {
    let terms = query_terms("ChatRoute extends BaseRoute override");

    assert_eq!(
        declaration_chunk_bonus(
            &terms,
            "export class ChatRoute extends BaseRoute {\n  override handle() {}\n}",
        ),
        5.75
    );
}

#[test]
fn declaration_bonus_requires_relationship_declaration_shape() {
    let terms = query_terms("BloomFilterPolicy inherits FilterPolicy KeyMayMatch override");

    assert_eq!(
        declaration_chunk_bonus(
            &terms,
            "bool BloomFilterPolicy::KeyMayMatch(const Slice& key, const Slice& filter) const {\n  return filter_policy_->KeyMayMatch(key, filter);\n}",
        ),
        0.0
    );
}

#[tokio::test]
async fn hybrid_chunks_rank_attached_symbol_identity() {
    let path = "src/routes/provider/openai.ts";
    let target_symbol = symbol(
        "target-symbol",
        "provider-file",
        path,
        "fromOpenaiChunk",
        range(40, 60),
    );
    let neighbor_symbol = symbol(
        "neighbor-symbol",
        "provider-file",
        path,
        "fromOpenaiRequest",
        range(1, 20),
    );
    let shared_content = "openai responses tool calls function_call_output convert common chunk";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file("provider-file", path, "typescript")],
        symbols: vec![neighbor_symbol, target_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "neighbor-chunk",
                "provider-file",
                path,
                shared_content,
                range(1, 20),
                Some("neighbor-symbol"),
            ),
            chunk(
                "target-chunk",
                "provider-file",
                path,
                shared_content,
                range(40, 60),
                Some("target-symbol"),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "openai responses tool calls function_call_output convert common chunk",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    let target = lexical_hit_score(&hits, 40).expect("target chunk should be recalled");
    let neighbor = lexical_hit_score(&hits, 1).expect("neighbor chunk should be recalled");
    assert!(
        target > neighbor,
        "attached symbol identity should rank target chunk above neighbor: {target} <= {neighbor}",
    );
}

#[tokio::test]
async fn hybrid_chunks_rank_execution_flow_context_above_local_tool_helpers() {
    let path = "packages/llm/src/protocols/openai-chat.ts";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file("protocol-file", path, "typescript")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "local-tool-helper",
                "protocol-file",
                path,
                "const lowerToolCall = (part) => ({ type: \"function\", function: part.name })",
                range(160, 166),
                None,
            ),
            chunk(
                "protocol-flow",
                "protocol-file",
                path,
                "OpenAI Chat protocol route uses SSE transport events.\nconst step = () => ToolStream.empty<number>()\nLifecycle.finish(lifecycle, events)",
                range(374, 392),
                None,
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].line_range.start, 374);
    assert!(hits[0].excerpt.contains("ToolStream.empty"));
}

#[tokio::test]
async fn hybrid_chunks_prefer_compact_high_coverage_usage() {
    let path = "samples/workflow/main.go";
    let compact = "func main() {\n\
\tc, err := client.Dial(envconfig.MustLoadDefaultClientOptions())\n\
\tif err != nil { panic(err) }\n\
\tw := worker.New(c, \"hello-world\", worker.Options{})\n\
\tw.RegisterWorkflow(helloworld.Workflow)\n\
\terr = w.Run(worker.InterruptCh())\n\
}";
    let verbose = (0..24)
        .map(|_| "client.Dial envconfig MustLoadDefaultClientOptions workflow client")
        .collect::<Vec<_>>()
        .join("\n");
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file("workflow-file", path, "go")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "verbose-chunk",
                "workflow-file",
                path,
                &verbose,
                range(1, 24),
                None,
            ),
            chunk(
                "compact-chunk",
                "workflow-file",
                path,
                compact,
                range(40, 47),
                None,
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].line_range.start, 40);
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn hybrid_chunks_prefer_complete_compact_api_sequences() {
    let complete_path = "samples/helloworld/worker/main.go";
    let partial_path = "samples/nexus/caller/worker/main.go";
    let verbose_path = "samples/worker-specific-task-queues/worker/main.go";
    let complete = "func main() {\n\
\tc, err := client.Dial(envconfig.MustLoadDefaultClientOptions())\n\
\tif err != nil { panic(err) }\n\
\tw := worker.New(c, \"hello-world\", worker.Options{})\n\
\tw.RegisterWorkflow(helloworld.Workflow)\n\
\tw.RegisterActivity(helloworld.Activity)\n\
\terr = w.Run(worker.InterruptCh())\n\
}";
    let partial = "func main() {\n\
\tc, err := client.Dial(clientOptions)\n\
\tw := worker.New(c, caller.TaskQueue, worker.Options{})\n\
\tw.RegisterWorkflow(caller.EchoCallerWorkflow)\n\
\tw.RegisterWorkflow(caller.HelloCallerWorkflow)\n\
\terr = w.Run(worker.InterruptCh())\n\
}";
    let verbose = (0..24)
        .map(|index| {
            if index % 2 == 0 {
                "w.RegisterWorkflow(flow.Workflow)"
            } else {
                "w.RegisterActivity(flow.Activity)"
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 3,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("complete-file", complete_path, "go"),
            file("partial-file", partial_path, "go"),
            file("verbose-file", verbose_path, "go"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "partial-chunk",
                "partial-file",
                partial_path,
                partial,
                range(20, 28),
                None,
            ),
            chunk(
                "verbose-chunk",
                "verbose-file",
                verbose_path,
                &verbose,
                range(80, 110),
                None,
            ),
            chunk(
                "complete-chunk",
                "complete-file",
                complete_path,
                complete,
                range(40, 49),
                None,
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    let summary = hits
        .iter()
        .map(|hit| format!("{}:{}={}", hit.path, hit.line_range.start, hit.score))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(hits[0].path, complete_path, "{summary}");
    let complete_score = lexical_hit_score(&hits, 40).expect("complete flow should be recalled");
    let partial_score = lexical_hit_score(&hits, 20).expect("partial flow should be recalled");
    assert!(complete_score > partial_score);
}

#[tokio::test]
async fn hybrid_chunks_prefer_multi_callback_operation_tables() {
    let generated_path = "src/generated_table.c";
    let ops_path = "src/driver_ops.c";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("generated-file", generated_path, "c"),
            file("ops-file", ops_path, "c"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "generated-table",
                "generated-file",
                generated_path,
                "static const struct rk_table_row rk_rows[] = {\n\
                    [RK_STAGE_READ] = {\n\
                        .name = \"read\",\n\
                        .read = rk_driver_read,\n\
                    },\n\
                };",
                range(15, 20),
                None,
            ),
            chunk(
                "driver-ops",
                "ops-file",
                ops_path,
                "const struct rk_driver_ops rk_default_ops = {\n\
                    .open = rk_driver_open,\n\
                    .read = rk_driver_read,\n\
                    .close = rk_driver_close,\n\
                };",
                range(26, 30),
                None,
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "operation table read callback dispatch designated initializer",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    let summary = hits
        .iter()
        .map(|hit| format!("{}:{}={}", hit.path, hit.line_range.start, hit.score))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(hits[0].path, ops_path, "{summary}");
    let ops_score = lexical_hit_score(&hits, 26).expect("operation table should be recalled");
    let generated_score = lexical_hit_score(&hits, 15).expect("generated table should be recalled");
    assert!(
        ops_score > generated_score,
        "operation table should outrank sparse generated table: {ops_score} <= {generated_score}",
    );
}

#[tokio::test]
async fn hybrid_chunks_rank_source_definition_bodies_above_declaration_surfaces() {
    let header_path = "include/store/cache.hpp";
    let source_path = "src/cache.cpp";
    let pipeline_path = "src/pipeline.cpp";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 3,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("header-file", header_path, "cpp"),
            file("source-file", source_path, "cpp"),
            file("pipeline-file", pipeline_path, "cpp"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "header-cache",
                "header-file",
                header_path,
                "template <typename Key>\n\
                class Cache {\n\
                 public:\n\
                    void Insert(const Key& key);\n\
                    virtual void Append(const std::string& key) = 0;\n\
                };",
                range(10, 18),
                None,
            ),
            chunk(
                "source-insert",
                "source-file",
                source_path,
                "template <typename Key>\n\
                void Cache<Key>::Insert(const Key& key)\n\
                {\n\
                    keys_.push_back(key);\n\
                    writer_->Append(std::string(key));\n\
                }",
                range(30, 36),
                None,
            ),
            chunk(
                "pipeline-lambda",
                "pipeline-file",
                pipeline_path,
                "int RunPipeline(Cache<std::string>& cache) {\n\
                    Pipeline pipeline;\n\
                    auto append_event = [&cache, &pipeline](const PipelineEvent& event) {\n\
                        cache.Insert(event.key);\n\
                        return pipeline(event);\n\
                    };\n\
                }",
                range(50, 58),
                None,
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "templated cache insert lambda pipeline writer append",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    let source = lexical_hit_score(&hits, 30).expect("source body should be recalled");
    let header = lexical_hit_score(&hits, 10).expect("header declaration should be recalled");
    assert!(
        source > header,
        "source definition body should outrank declaration-only surface: {source} <= {header}",
    );
}

#[tokio::test]
async fn hybrid_chunks_rank_local_query_term_proximity_above_scattered_matches() {
    let broad_path = "db/version_set.h";
    let focused_path = "db/db_impl.h";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("broad-file", broad_path, "cpp"),
            file("focused-file", focused_path, "cpp"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "broad-chunk",
                "broad-file",
                broad_path,
                "class VersionSet {\n\
                 public:\n\
                    Status Recover(bool* save_manifest);\n\
                    void MarkFileNumberUsed(uint64_t number);\n\
                    uint64_t ManifestFileNumber() const;\n\
                    void AddLiveFiles(std::set<uint64_t>* live);\n\
                    void AppendVersion(Version* version);\n\
                    void Finalize(Version* version);\n\
                    uint64_t NewFileNumber();\n\
                    uint64_t LastSequence() const;\n\
                    void SetLastSequence(uint64_t sequence);\n\
                    std::string descriptor_name_;\n\
                };",
                range(10, 18),
                None,
            ),
            chunk(
                "focused-chunk",
                "focused-file",
                focused_path,
                "class DBImpl {\n\
                 private:\n\
                    // Recover the descriptor and append updates to the edit.\n\
                    Status Recover(VersionEdit* edit, bool* save_manifest);\n\
                };",
                range(40, 45),
                None,
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "Recover descriptor save_manifest VersionEdit",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    let focused =
        lexical_hit_score(&hits, 40).expect("focused declaration chunk should be recalled");
    let broad = lexical_hit_score(&hits, 10).expect("broad declaration chunk should be recalled");
    assert!(
        focused > broad,
        "localized query evidence should outrank scattered class evidence: {focused} <= {broad}",
    );
}

#[tokio::test]
async fn hybrid_chunks_rank_exact_path_above_mention_only_hits() {
    let target_path = "src/runtime/config.ts";
    let noise_path = "aaa/noise.ts";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("noise-file", noise_path, "typescript"),
            file("target-file", target_path, "typescript"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "noise-chunk",
                "noise-file",
                noise_path,
                "See src/runtime/config.ts for runtime configuration.",
                range(1, 3),
                None,
            ),
            chunk(
                "target-chunk",
                "target-file",
                target_path,
                "const defaults = loadRuntimeSettings();",
                range(10, 12),
                None,
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(target_path, CodeQueryKind::Hybrid))
        .await
        .expect("path hybrid query should succeed");

    assert_eq!(hits[0].path, target_path);
    let target = lexical_hit_score(&hits, 10).expect("target chunk should be recalled");
    let noise = lexical_hit_score(&hits, 1).expect("noise chunk should be recalled");
    assert!(
        target > noise,
        "exact chunk path should outrank mention-only hit: {target} <= {noise}",
    );
}

fn lexical_hit_score(hits: &[CodeRetrievalHit], line_start: u32) -> Option<f64> {
    hits.iter()
        .find(|hit| {
            hit.line_range.start == line_start
                && hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
        })
        .map(|hit| hit.score)
}

fn request(query: &str, kind: CodeQueryKind) -> crate::domain::CodeRetrievalRequest {
    let selector =
        crate::domain::CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

fn file(file_id: &str, path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 80,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("function {name}()"),
        doc_comment: None,
        byte_range: range(line_range.start, line_range.end),
        line_range,
        symbol_role: None,
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    line_range: RepositoryCodeRange,
    symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        content: content.to_owned(),
        byte_range: range(line_range.start, line_range.end),
        line_range,
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}
