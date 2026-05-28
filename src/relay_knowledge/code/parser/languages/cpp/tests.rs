use crate::domain::{CodeParseStatus, CodeRepositoryRegistration};

use super::*;

#[test]
fn cpp_macro_decorated_classes_can_recover_as_parsed() {
    let snapshot = parse_source_snapshot(
        "include/http_module.hpp",
        br#"
#define RK_CPP_API __attribute__((visibility("default")))

class BaseModule {
 public:
    virtual ~BaseModule() = default;
};

RK_CPP_API class HttpModule final : public BaseModule {
 public:
    void Run();
};
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "HttpModule" && symbol.kind == "class"),
        "macro-decorated class should expose the real class name: {:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_post_keyword_decorators_and_uppercase_type_names_recover_as_parsed() {
    let snapshot = parse_source_snapshot(
        "include/exported.hpp",
        br#"
class __declspec(dllexport) HTTP_MODULE final {
 public:
    void Run();
};
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "HTTP_MODULE" && symbol.kind == "class"),
        "post-keyword decorators should not hide uppercase snake-case type names: {:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_digit_decorated_type_recovery_accepts_uppercase_type_names() {
    let snapshot = parse_source_snapshot(
        "include/http_module.hpp",
        br#"
#define RK2_API __attribute__((visibility("default")))

RK2_API class HTTP_MODULE final {
 public:
    void Run();
};
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "HTTP_MODULE" && symbol.kind == "class"),
        "digit-suffixed decorator macros should still recover the real type name: {:?}",
        snapshot.symbols
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "RK2_API" && symbol.kind == "class"),
        "decorator tokens should not replace the real C++ type name"
    );
}

#[test]
fn cpp_manual_recovery_keeps_function_definitions_with_type_parameters() {
    let snapshot = parse_source_snapshot(
        "src/http_module.cpp",
        br#"
int BuildResponse(struct Request *request)
{
    return request != nullptr;
}
"#,
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "BuildResponse" && symbol.kind == "function"),
        "function definitions should not be replaced by parameter type recovery: {:?}",
        snapshot.symbols
    );
    assert_eq!(
        snapshot
            .symbols
            .iter()
            .filter(|symbol| symbol.name == "Request" && symbol.kind == "type")
            .count(),
        0,
        "parameter type mentions should not synthesize type definitions"
    );
}

#[test]
fn cpp_manual_type_recovery_rejects_keywords_and_type_mentions() {
    let snapshot = parse_source_snapshot(
        "src/direction.cpp",
        br#"
enum class Direction {
    Left,
    Right,
};

int Parse(enum Direction direction);
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Direction" && symbol.kind == "type"),
        "enum class declarations should expose the real type name: {:?}",
        snapshot.symbols
    );
    assert!(
        !snapshot.symbols.iter().any(|symbol| symbol.name == "class"),
        "C++ keywords should not become recovered type names"
    );
    let mention = "int Parse(enum Direction direction);";
    let language = detect_language("src/direction_mention.cpp").expect("C++ should be configured");
    let parsed = parse_tree(language, mention).expect("C++ should parse");
    let declaration_node = first_node_of_kind(parsed.root_node(), "declaration")
        .expect("function declaration should be present");
    let manual = manual_definitions(mention, language.id, declaration_node);
    assert!(
        !manual
            .iter()
            .any(|(name, kind, _)| name == "Direction" && *kind == "type"),
        "manual recovery should not turn type mentions into type definitions: {:?}",
        manual
    );
}

#[test]
fn cpp_manual_type_recovery_rejects_builtin_specifier_candidates() {
    let content = "struct { int value; } node;";
    let language = detect_language("src/anonymous.cpp").expect("C++ should be configured");
    let parsed = parse_tree(language, content).expect("C++ should parse");
    let declaration_node = first_node_of_kind(parsed.root_node(), "declaration")
        .expect("anonymous struct declaration should be present");
    let manual = manual_definitions(content, language.id, declaration_node);

    assert!(
        !manual
            .iter()
            .any(|(name, kind, _)| name == "int" && *kind == "type"),
        "builtin specifiers should not become recovered type symbols: {:?}",
        manual
    );
}

#[test]
fn cpp_union_specifiers_are_manual_definition_candidates() {
    let snapshot = parse_source_snapshot(
        "src/payload.cpp",
        br#"
union Payload {
    int value;
};
"#,
    );

    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Payload" && symbol.kind == "type"),
        "union_specifier traversal should recover union type symbols: {:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_decorated_enum_union_errors_can_recover_as_parsed() {
    let snapshot = parse_source_snapshot(
        "include/modes.hpp",
        br#"
#define RK2_API __attribute__((visibility("default")))

RK2_API enum Mode {
    Fast,
    Slow,
};

RK2_API union Payload {
    int value;
};
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    for name in ["Mode", "Payload"] {
        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.name == name && symbol.kind == "type"),
            "{name} should be recovered from decorated enum/union syntax: {:?}",
            snapshot.symbols
        );
    }
}

#[test]
fn cpp_explicit_template_instantiations_can_recover_as_parsed() {
    let snapshot = parse_source_snapshot(
        "src/cache.cpp",
        br#"
#include <memory>
#include <string>

namespace rk::store {

class Writer {
 public:
    void Append(const std::string& key);
};

template <typename Key>
class Cache {
 public:
    explicit Cache(std::unique_ptr<Writer> writer);
    void Insert(const Key& key);

 private:
    std::unique_ptr<Writer> writer_;
};

template <typename Key>
Cache<Key>::Cache(std::unique_ptr<Writer> writer) : writer_(std::move(writer)) {}

template <typename Key>
void Cache<Key>::Insert(const Key& key)
{
    writer_->Append(std::string(key));
}

template class Cache<std::string>;

}  // namespace rk::store
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Cache" || symbol.name == "Cache<Key>::Insert"),
        "template implementation should keep structured symbols: {:?}",
        snapshot.symbols
    );
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
