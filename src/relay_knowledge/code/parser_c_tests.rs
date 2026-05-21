use crate::domain::CodeRepositoryRegistration;

use super::*;

#[test]
fn c_functions_use_body_ranges_for_call_graph_ownership() {
    let snapshot = parse_source_snapshot(
        "mm/cma_debug.c",
        br#"
static void cma_debugfs_add_one(void)
{
    debugfs_create_dir("ranges", NULL);
}

static int cma_debugfs_init(void)
{
    cma_debugfs_add_one();
    return 0;
}
"#,
    );
    let add_one = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "cma_debugfs_add_one")
        .expect("C function definition should be indexed");
    let init_call = snapshot
        .calls
        .iter()
        .find(|call| call.callee_name == "cma_debugfs_add_one")
        .expect("call should be indexed");

    assert_eq!(add_one.kind, "function");
    assert!(
        add_one.line_range.end > add_one.line_range.start,
        "function definitions should cover their body"
    );
    assert_eq!(init_call.caller_name.as_deref(), Some("cma_debugfs_init"));
    assert!(init_call.caller_symbol_snapshot_id.is_some());
}

#[test]
fn c_macros_are_indexed_and_macro_calls_resolve_to_them() {
    let snapshot = parse_source_snapshot(
        "include/linux/container_of.h",
        br#"
#define container_of(ptr, type, member) ({ ptr; })

void use_macro(void)
{
    container_of(ptr, struct task_struct, member);
}
"#,
    );
    let macro_symbol = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "container_of")
        .expect("macro definition should be indexed");
    let macro_call = snapshot
        .references
        .iter()
        .find(|reference| reference.name == "container_of")
        .expect("macro-style call should be indexed");

    assert_eq!(macro_symbol.kind, "macro");
    assert_eq!(macro_call.resolution_state, "resolved");
    assert_eq!(
        macro_call.target_symbol_snapshot_id.as_deref(),
        Some(macro_symbol.symbol_snapshot_id.as_str())
    );
}

#[test]
fn c_linux_syscall_define_macros_are_indexed_as_function_definitions() {
    let snapshot = parse_source_snapshot(
        "fs/read_write.c",
        br#"
SYSCALL_DEFINE3(read, unsigned int, fd, char __user *, buf, size_t, count)
{
    return ksys_read(fd, buf, count);
}
"#,
    );

    let syscall = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "read")
        .expect("SYSCALL_DEFINE should expose the syscall name as a function definition");

    assert_eq!(syscall.kind, "function");
    assert_eq!(syscall.line_range.start, 2);
}

#[test]
fn c_includes_resolve_to_indexed_header_files() {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        2,
        0,
    );
    parse_indexed_file(
        &mut build,
        "include/linux/debugfs.h",
        br#"
struct dentry;
"#,
    )
    .expect("header should parse");
    parse_indexed_file(
        &mut build,
        "mm/cma_debug.c",
        br#"
#include <linux/debugfs.h>

void init_debugfs(void) {}
"#,
    )
    .expect("source should parse");

    let snapshot = build.finish();
    let include = snapshot
        .imports
        .iter()
        .find(|import| import.module.contains("linux/debugfs.h"))
        .expect("C include should be indexed");

    assert_eq!(include.resolution_state, "resolved");
    assert_eq!(
        include.target_hint.as_deref(),
        Some("include/linux/debugfs.h")
    );
    assert_eq!(include.confidence_tier, "inferred");
}

#[test]
fn c_header_decorated_cpp_class_uses_real_type_name() {
    let snapshot = parse_source_snapshot(
        "include/leveldb/filter_policy.h",
        br#"
namespace leveldb {

class LEVELDB_EXPORT FilterPolicy {
 public:
  virtual bool KeyMayMatch() const = 0;
};

}
"#,
    );
    let file = snapshot
        .files
        .iter()
        .find(|file| file.path == "include/leveldb/filter_policy.h")
        .expect("header file should be indexed");

    assert_eq!(file.language_id, "c");
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "FilterPolicy" && symbol.kind == "class" })
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "LEVELDB_EXPORT" && symbol.kind == "function" }),
        "export macros should not replace the real C++ class name"
    );
}

#[test]
fn c_top_level_composite_initializers_are_retrievable_constant_symbols() {
    let snapshot = parse_source_snapshot(
        "mm/page_idle.c",
        br#"
static int scalar_flag = IS_ENABLED(CONFIG_PAGE_IDLE);

static const struct vm_operations_struct special_mapping_vmops = {
    .close = special_mapping_close,
    .fault = special_mapping_fault,
    .mremap = special_mapping_mremap,
};

static const struct bin_attribute page_idle_bitmap_attr =
        __BIN_ATTR(bitmap, 0600, page_idle_bitmap_read, page_idle_bitmap_write, 0);
"#,
    );

    for name in ["special_mapping_vmops", "page_idle_bitmap_attr"] {
        let symbol = snapshot
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("{name} should be indexed as retrievable data"));
        assert_eq!(symbol.kind, "constant");
    }
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "scalar_flag"),
        "scalar macro initializers should not create broad top-level data noise"
    );
    assert!(snapshot.chunks.iter().any(|chunk| {
        chunk.content.contains("special_mapping_vmops")
            && chunk.content.contains(".fault = special_mapping_fault")
    }));
    assert!(snapshot.chunks.iter().any(|chunk| {
        chunk.content.contains("page_idle_bitmap_attr") && chunk.content.contains("__BIN_ATTR")
    }));
}

#[test]
fn c_function_pointer_declarations_are_not_function_symbols() {
    let snapshot = parse_source_snapshot(
        "include/linux/callbacks.h",
        br#"
int (*handler)(void);
int *returns_pointer(void);
int declared(void);
"#,
    );

    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "handler"),
        "function pointer variables should not be indexed as function declarations"
    );
    assert!(snapshot.symbols.iter().any(|symbol| {
        symbol.name == "returns_pointer" && symbol.kind == "function_declaration"
    }));
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "declared" && symbol.kind == "function_declaration")
    );
}

#[test]
fn c_function_pointer_parameters_are_not_global_function_symbols() {
    let snapshot = parse_source_snapshot(
        "include/linux/callbacks.h",
        br#"
int (*handler)(int cb(int));
int accepts_callback(int cb(int));
"#,
    );

    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "handler"),
        "function pointer variables should not be indexed"
    );
    assert!(
        !snapshot.symbols.iter().any(|symbol| symbol.name == "cb"),
        "function declarators nested inside parameters should not become global functions"
    );
    assert!(snapshot.symbols.iter().any(|symbol| {
        symbol.name == "accepts_callback" && symbol.kind == "function_declaration"
    }));
}

#[test]
fn c_typedef_function_types_are_type_symbols_not_callable_functions() {
    let content = r#"
typedef int comparison_fn_t(const void *, const void *);
typedef int (*callback_fn_t)(void);
int compare_values(const void *, const void *);
"#;
    let snapshot = parse_source_snapshot("include/linux/comparison.h", content.as_bytes());

    for name in ["comparison_fn_t", "callback_fn_t"] {
        let alias = snapshot
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("{name} should be indexed as a type alias"));
        assert_eq!(alias.kind, "type");
        assert!(
            !snapshot.symbols.iter().any(|symbol| {
                symbol.name == name
                    && matches!(symbol.kind.as_str(), "function" | "function_declaration")
            }),
            "typedef aliases should not be indexed as callable functions"
        );
    }
    assert!(snapshot.symbols.iter().any(|symbol| {
        symbol.name == "compare_values" && symbol.kind == "function_declaration"
    }));
}

#[test]
fn c_function_declarations_can_return_function_pointers() {
    let snapshot = parse_source_snapshot(
        "include/linux/signals.h",
        br#"
void (*signal(int sig, void (*handler)(int)))(int);
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "signal" && symbol.kind == "function_declaration" })
    );
}

#[test]
fn c_multi_declaration_prototypes_index_each_function() {
    let snapshot = parse_source_snapshot(
        "include/linux/prototypes.h",
        br#"
int first(void), second(void);
"#,
    );
    let declarations = snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == "function_declaration")
        .map(|symbol| symbol.name.as_str())
        .collect::<Vec<_>>();

    assert!(declarations.contains(&"first"));
    assert!(declarations.contains(&"second"));
}

#[test]
fn cpp_same_line_overloads_keep_distinct_symbols() {
    let snapshot = parse_source_snapshot(
        "src/overload.cpp",
        br#"
void f(int); void f(double);
"#,
    );
    let overloads = snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "f")
        .count();

    assert_eq!(overloads, 2);
}

#[test]
fn c_initializer_and_subscripted_function_pointer_uses_are_references() {
    let snapshot = parse_source_snapshot(
        "src/dispatch.c",
        br#"
struct rk_device;
typedef int (*rk_stage_fn)(struct rk_device *dev);
int rk_validate_device(struct rk_device *dev);
int rk_driver_read(struct rk_device *dev);

static rk_stage_fn rk_pipeline[] = {
    rk_validate_device,
};

static const struct rk_table_row {
    rk_stage_fn read;
} rk_rows[] = {
    [0] = {
        .read = rk_driver_read,
    },
};

int rk_run_pipeline(struct rk_device *dev)
{
    return rk_pipeline[0](dev) + rk_rows[0].read(dev);
}
"#,
    );

    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "rk_driver_read" && reference.kind == "implementation"
    }));
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "rk_pipeline"),
        "function pointer arrays should remain searchable by their callable identifier"
    );
    assert!(
        snapshot
            .calls
            .iter()
            .any(|call| call.callee_name == "rk_pipeline"),
        "subscripted function pointer calls should use the array identifier, not the index"
    );
    assert!(
        !snapshot.calls.iter().any(|call| call.callee_name == "0"),
        "subscript arguments should not replace the callable identifier"
    );
}

#[test]
fn cpp_type_and_namespace_alias_uses_are_references() {
    let snapshot = parse_source_snapshot(
        "src/cache.cpp",
        br#"
#include <memory>
#include <string>
#include <vector>

namespace rk::store {

template <typename Key>
class Cache {
 public:
    using KeyList = std::vector<Key>;

 private:
    KeyList keys_;
};

namespace cache_alias = rk::store;

std::unique_ptr<Cache<std::string>> BuildCache()
{
    return std::make_unique<cache_alias::Cache<std::string>>();
}

}  // namespace rk::store
"#,
    );

    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "KeyList" && reference.kind == "type"),
        "field declarations should expose type-alias uses as type references"
    );
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "cache_alias" && reference.kind == "implementation"
    }));
}

#[test]
fn cpp_function_definitions_own_calls_inside_namespaces() {
    let snapshot = parse_source_snapshot(
        "db/db_impl.cc",
        br#"
namespace leveldb {

Options SanitizeOptions(const Options& src)
{
    Options result;
    result.block_cache = NewLRUCache(8 << 20);
    return result;
}

}
"#,
    );

    let function = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "SanitizeOptions")
        .expect("C++ function definition should be indexed");
    let call = snapshot
        .calls
        .iter()
        .find(|call| call.callee_name == "NewLRUCache")
        .expect("C++ call should be indexed");

    assert_eq!(function.kind, "function");
    assert_eq!(call.caller_name.as_deref(), Some("SanitizeOptions"));
}

#[test]
fn cpp_enum_tag_and_manual_fallback_deduplicate() {
    let snapshot = parse_source_snapshot(
        "db/db_iter.cc",
        br#"
class DBIter {
 public:
  enum Direction { kForward, kReverse };
};
"#,
    );
    let directions = snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "Direction")
        .collect::<Vec<_>>();

    assert_eq!(directions.len(), 1);
    assert_eq!(directions[0].kind, "type");
}

#[test]
fn non_c_function_definitions_keep_generic_manual_fallback() {
    let content = r#"
def retry_policy():
    return 1
"#;
    let language = detect_language("src/app.py").expect("python should be configured");
    let parsed = parse_tree(language, content).expect("python should parse");
    let function_node = first_node_of_kind(parsed.root_node(), "function_definition")
        .expect("function node should be present");

    let definitions = manual_definitions(content, language.id, function_node);

    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].0, "retry_policy");
    assert_eq!(definitions[0].1, "function");
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> crate::domain::CodeIndexSnapshot {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    parse_indexed_file(&mut build, path, source).expect("file should parse");

    build.finish()
}

fn first_node_of_kind<'tree>(
    root: tree_sitter::Node<'tree>,
    kind: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return Some(node);
        }
        push_children_reverse(node, &mut stack);
    }

    None
}
