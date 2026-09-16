use crate::{
    code::{SnapshotBuild, parser::parse_indexed_file},
    domain::CodeRepositoryRegistration,
};

#[test]
fn cpp_recovered_local_types_keep_the_real_enclosing_function_boundary() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "local.cpp",
        "void outer() { class API_EXPORT Local { public: void run() { target(); } }; }",
    )]);
    let local = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Local")
        .unwrap();
    assert!(
        local
            .type_owner
            .as_ref()
            .unwrap()
            .target_hint
            .starts_with("local@")
    );
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.kind == "class" && symbol.name == "API_EXPORT")
    );
}

#[test]
fn cpp_class_declarations_with_instances_keep_their_structured_type_owner() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "owner.cpp",
        "class Owner { public: int n; void run() { target(); } } instance{outside()};",
    )]);
    let run = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "run")
        .unwrap();
    let member = run.type_owner.as_ref().unwrap();
    assert_eq!(member.target_hint, "Owner");
    assert!(
        snapshot
            .calls
            .iter()
            .any(|call| call.callee_name == "target" && call.caller_name.as_deref() == Some("run"))
    );
    let outside: Vec<_> = snapshot
        .calls
        .iter()
        .filter(|call| call.callee_name == "outside")
        .collect();
    assert!(!outside.is_empty(), "{:?}", snapshot.calls);
    assert!(
        outside
            .iter()
            .all(|call| call.caller_name.as_deref() != Some("Owner"))
    );
    assert!(
        snapshot.symbols.iter().any(|symbol| symbol.name == "Owner"
            && symbol.type_owner.as_ref().is_some_and(
                |owner| owner.relation == "declaration" && owner.identity == member.identity
            )),
        "{:?}",
        snapshot.symbols
    );
}

#[test]
fn cpp_recovered_declaration_types_have_ownership_and_large_namespace_heads_stay_unknown() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "modes.cpp",
        "API_EXPORT enum Mode { Fast, Slow }; API_EXPORT union Payload { int value; };",
    )]);
    for name in ["Mode", "Payload"] {
        assert!(
            snapshot.symbols.iter().any(|symbol| symbol.name == name
                && symbol
                    .type_owner
                    .as_ref()
                    .is_some_and(|owner| owner.relation == "declaration")),
            "{:?}",
            snapshot.symbols
        );
    }
    let source = format!(
        "API_BEGIN\nnamespace {} detail {{ class Owner {{}}; }}",
        " ".repeat(4096)
    );
    let snapshot = crate::code::syntax_snapshot_for_tests(&[("owner.cpp", &source)]);
    assert!(
        snapshot
            .symbols
            .iter()
            .filter_map(|symbol| symbol.type_owner.as_ref())
            .all(|owner| !owner.target_hint.contains("macro@API_BEGIN"))
    );
}

#[test]
fn flow_annotated_jsx_retains_class_calls_and_reports_syntax_errors() {
    let source = "// @flow\ntype State = { value: ?number };\nexport default class Widget extends Base<Props, State> { run: () => void = () => { target(); }; }";
    let snapshot = crate::code::syntax_snapshot_for_tests(&[("widget.jsx", source)]);
    assert!(
        snapshot.symbols.iter().any(|s| s.name == "Widget"
            && s.type_owner
                .as_ref()
                .is_some_and(|o| o.relation == "declaration")),
        "{:?}",
        snapshot.symbols
    );
    assert!(
        snapshot
            .calls
            .iter()
            .any(|c| c.callee_name == "target" && c.caller_name.as_deref() == Some("run")),
        "{:?}",
        snapshot.calls
    );
    let broken = format!("{source}\nconst broken = ;");
    let snapshot = crate::code::syntax_snapshot_for_tests(&[("widget.jsx", &broken)]);
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.parse_status == crate::domain::CodeParseStatus::Partial)
    );
}

#[test]
fn unknown_receivers_do_not_inherit_unrelated_method_targets() {
    for (path, source, name, hint) in [
        (
            "app.js",
            "class Headers { has() {} } function visit() { const keys = new Set(); return keys.has('x'); }",
            "has",
            "keys.has",
        ),
        (
            "app.go",
            "package app\nimport \"reflect\"\ntype Source struct{}\nfunc (s Source) Key() string {return \"\"}\nfunc visit(t reflect.Type) {t.Key()}\n",
            "Key",
            "t.Key",
        ),
        (
            "app.py",
            "class Headers:\n def has(self): pass\ndef visit(keys):\n return keys.has('x')\n",
            "has",
            "keys.has",
        ),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[(path, source)]);
        let calls: Vec<_> = snapshot
            .references
            .iter()
            .filter(|r| r.kind == "call" && r.name == name)
            .collect();
        assert!(!calls.is_empty(), "{path}: {:?}", snapshot.references);
        assert!(
            calls.iter().all(|r| r.target_symbol_snapshot_id.is_none()
                && r.resolution_state == "unresolved"
                && r.target_hint.as_deref() == Some(hint)),
            "{calls:?}"
        );
    }
}

#[test]
fn nested_anonymous_bodies_have_local_call_owners() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "app.jsx",
        "class Buffer { proxy() { return (...args) => { this.items.push(args); }; } }",
    )]);
    let calls: Vec<_> = snapshot
        .calls
        .iter()
        .filter(|call| call.callee_name.ends_with("push"))
        .collect();
    assert!(!calls.is_empty(), "{:?}", snapshot.calls);
    assert!(
        calls.iter().all(|call| call
            .caller_name
            .as_deref()
            .is_some_and(|name| name.starts_with("anonymous@"))),
        "{calls:?}"
    );
    assert!(
        snapshot
            .symbols
            .iter()
            .filter(|s| s.name.starts_with("anonymous@"))
            .all(|s| s.type_owner.is_none())
    );
}

#[test]
fn closure_owners_preserve_named_bindings_and_cover_initializer_callbacks() {
    for (source, expected) in [
        ("function outer(){const inner=()=>target();}", "inner"),
        (
            "function outer(){const inner=function(){target();};}",
            "inner",
        ),
        (
            "class C { values = items.map(() => target()); }",
            "anonymous@",
        ),
        (
            "class C { run(){return function*(){target();};} }",
            "anonymous@",
        ),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[("app.js", source)]);
        assert!(
            snapshot
                .symbols
                .iter()
                .filter(|symbol| symbol.name.starts_with("anonymous@"))
                .all(|symbol| symbol.type_owner.is_none()),
            "{source}: {:?}",
            snapshot.symbols
        );
        let calls: Vec<_> = snapshot
            .calls
            .iter()
            .filter(|call| call.callee_name == "target")
            .collect();
        assert!(!calls.is_empty());
        assert!(
            calls.iter().all(|call| call
                .caller_name
                .as_deref()
                .is_some_and(|name| name.starts_with(expected))),
            "{source}: {calls:?}"
        );
    }
}

#[test]
fn receiver_spelling_does_not_prove_this_or_self_binding() {
    for (path, source) in [
        (
            "app.py",
            "class C:\n def has(self): pass\n def run(self, foreign):\n  self = foreign\n  return self.has()\n",
        ),
        ("app.js", "class C {has() {} run(self){return self.has();}}"),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[(path, source)]);
        assert!(
            snapshot
                .references
                .iter()
                .filter(|r| r.kind == "call" && r.name == "has")
                .all(|r| r.target_symbol_snapshot_id.is_none()),
            "{:?}",
            snapshot.references
        );
    }
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "app.js",
        "class C {has() {} run(){return this.has();}}",
    )]);
    assert!(
        snapshot
            .references
            .iter()
            .any(|r| r.kind == "call" && r.name == "has" && r.target_symbol_snapshot_id.is_some()),
        "{:?}",
        snapshot.references
    );
}

#[test]
fn this_receiver_requires_matching_static_or_instance_members() {
    for (source, resolved) in [
        ("class C {static run(){this.has();} has(){}}", false),
        ("class C {run(){this.has();} static has(){}}", false),
        ("class C {static run(){this.has();} static has(){}}", true),
        ("class C {run(){this.has();} has(){}}", true),
        ("class C extends this.has() {has(){}}", false),
        ("class C {has(){} [this.has()](){}}", false),
        ("class C {static {this.has();} has(){}}", false),
        ("class C {static value=this.has(); has(){}}", false),
        ("class C {static {this.has();} static has(){}}", true),
        ("class C {static value=this.has(); static has(){}}", true),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[("app.js", source)]);
        let calls: Vec<_> = snapshot
            .references
            .iter()
            .filter(|reference| reference.kind == "call" && reference.name == "has")
            .collect();
        assert!(!calls.is_empty());
        assert!(
            calls
                .iter()
                .all(|reference| reference.target_symbol_snapshot_id.is_some() == resolved),
            "{source}: {calls:?}"
        );
    }
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "C.java",
        "class C {static void has(){} void run(){this.has();}}",
    )]);
    assert!(
        snapshot
            .references
            .iter()
            .any(|r| r.kind == "call" && r.name == "has" && r.target_symbol_snapshot_id.is_some())
    );
}

#[test]
fn code_index_persistence_performance_suite_receiver_hints_remain_bounded() {
    let source = format!("function run(){{ builder{}; }}", ".step()".repeat(200));
    let snapshot = crate::code::syntax_snapshot_for_tests(&[("app.js", &source)]);
    let references: Vec<_> = snapshot
        .references
        .iter()
        .filter(|r| r.kind == "call")
        .collect();
    assert!(references.len() >= 200);
    assert!(references.iter().all(|reference| {
        reference
            .target_hint
            .as_ref()
            .is_none_or(|hint| hint.len() <= 513)
    }));
}

#[test]
fn swift_requirements_constructors_and_subscripts_have_member_ranges() {
    let source = "protocol Contract {func request()}\nclass Owner {\n init() {target()}\n subscript(i: Int) -> Int {return target()}\n func target() -> Int {0}\n}\n";
    let snapshot = crate::code::syntax_snapshot_for_tests(&[("owner.swift", source)]);
    for (name, expected_owner) in [
        ("request", "Contract"),
        ("init", "Owner"),
        ("subscript", "Owner"),
    ] {
        let member = snapshot
            .symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("{name}: {:?}", snapshot.symbols));
        let owner = member.type_owner.as_ref().unwrap();
        assert_eq!(owner.relation, "direct_member");
        assert_eq!(owner.target_hint, expected_owner);
        assert!(
            !source[member.byte_range.start as usize..member.byte_range.end as usize]
                .starts_with("class")
        );
    }
}

#[test]
fn type_ownership_cpp_templates_keep_primary_and_concrete_identities_separate() {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(
        "owner.cpp",
        "struct V {}; template<class T> class Owner {public: void run(); template<class U> void member();}; template<class U> void Owner<U>::run() {} template<> class Owner<int> {public: void extra();}; void Owner<int>::extra() {} template<> class Owner<V> {}; template<class V> template<class U> void Owner<V>::member() {}",
    )]);
    for (member, hint) in [
        ("run", "Owner<@0>"),
        ("extra", "Owner<int>"),
        ("member", "Owner<@0>"),
    ] {
        let owner = snapshot
            .symbols
            .iter()
            .find(|s| s.name == member && s.signature.contains('{'))
            .and_then(|s| s.type_owner.as_ref())
            .unwrap_or_else(|| panic!("{member}: {:?}", snapshot.symbols));
        assert_eq!(owner.target_hint, hint);
        assert_eq!(owner.resolution_state.as_deref(), Some("resolved"));
    }
}

#[test]
fn exported_types_and_callable_fields_keep_their_ast_owner() {
    for (path, source) in [
        ("owner.js", "export class Owner {run=()=>{target()};}"),
        ("owner.ts", "export class Owner {run=()=>{target()};}"),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[(path, source)]);
        let declaration = snapshot
            .symbols
            .iter()
            .find(|s| s.name == "Owner")
            .and_then(|s| s.type_owner.as_ref())
            .unwrap_or_else(|| panic!("{:?}", snapshot.symbols));
        let member = snapshot
            .symbols
            .iter()
            .find(|s| s.name == "run")
            .and_then(|s| s.type_owner.as_ref())
            .unwrap_or_else(|| panic!("{:?}", snapshot.symbols));
        assert_eq!(member.identity, declaration.identity);
        assert_eq!(member.relation, "direct_member");
    }
}

#[test]
fn portable_extensions_and_companions_preserve_direct_ownership() {
    for (path, source, member, expected) in [
        (
            "owner.kt",
            "class Owner {}\nfun Owner.run() {}",
            "run",
            "Owner",
        ),
        (
            "owner.scala",
            "class Owner {}\nextension (owner: Owner) { def run(): Unit = {} }",
            "run",
            "Owner",
        ),
        (
            "owner.kt",
            "class Owner {\n companion object {\n fun run() {}\n }\n}",
            "run",
            "Owner.Companion",
        ),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[(path, source)]);
        let owner = snapshot
            .symbols
            .iter()
            .find(|s| s.name == member)
            .and_then(|s| s.type_owner.as_ref())
            .unwrap_or_else(|| panic!("{path}: {:?}", snapshot.symbols));
        assert_eq!(owner.target_hint, expected);
        assert_eq!(owner.resolution_state.as_deref(), Some("resolved"));
    }
}

#[test]
fn portable_type_identity_and_swift_ranges_preserve_real_declarations() {
    let body = "        target()\n".repeat(300);
    let swift = format!(
        "class Owner {{\n    func run() {{\n{body}    }}\n}}\nextension Owner {{\n    func extra() {{}}\n}}\n"
    );
    let snapshot = crate::code::syntax_snapshot_for_tests(&[("owner.swift", &swift)]);
    let run: Vec<_> = snapshot
        .symbols
        .iter()
        .filter(|s| s.name == "run")
        .collect();
    assert_eq!(run.len(), 1, "{:?}", snapshot.symbols);
    assert!(run[0].signature.len() <= 512);
    let declarations: Vec<_> = snapshot
        .symbols
        .iter()
        .filter_map(|s| s.type_owner.as_ref())
        .filter(|o| o.relation == "declaration")
        .collect();
    assert_eq!(declarations.len(), 1);
    let extra = snapshot
        .symbols
        .iter()
        .find(|s| s.name == "extra")
        .unwrap()
        .type_owner
        .as_ref()
        .unwrap();
    assert_eq!(extra.identity, declarations[0].identity);
    assert_eq!(extra.basis.as_deref(), Some("swift_extension"));
    let snapshot = crate::code::syntax_snapshot_for_tests(&[
        ("a.py", "class Owner:\n    def run(self): pass\n"),
        ("b.py", "class Owner:\n    def run(self): pass\n"),
    ]);
    let owners: std::collections::BTreeSet<_> = snapshot
        .symbols
        .iter()
        .filter(|s| s.name == "Owner")
        .map(|s| s.type_owner.as_ref().unwrap().identity.clone())
        .collect();
    assert_eq!(owners.len(), 2);
}

#[test]
fn ruby_singletons_require_the_lexical_type_receiver() {
    for source in [
        "class Owner\n OTHER=Object.new\n def OTHER.run\n end\nend\n",
        "class Owner\n class << OTHER\n  def run\n  end\n end\nend\n",
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[("owner.rb", source)]);
        assert!(
            snapshot
                .symbols
                .iter()
                .filter(|s| s.name == "run")
                .all(|s| s.type_owner.is_none()),
            "{:?}",
            snapshot.symbols
        );
    }
}

#[test]
fn direct_type_ownership_is_extracted_across_language_grammars() {
    let mut failures = Vec::new();
    for (path, source) in [
        ("Owner.java", "class Owner { void run() {} }"),
        (
            "owner.py",
            "class Owner:\n    def run(self):\n        pass\n",
        ),
        ("owner.js", "class Owner { run() {} }"),
        ("owner.jsx", "class Owner { run() {} }"),
        ("owner.ts", "class Owner { run(): void {} }"),
        ("owner.tsx", "class Owner { run(): void {} }"),
        ("owner.cpp", "class Owner { public: void run() {} };"),
        ("Owner.cs", "class Owner { public void run() {} }"),
        ("owner.rs", "struct Owner; impl Owner { fn run(&self) {} }"),
        (
            "owner.go",
            "package demo\ntype Owner struct {}\nfunc (o Owner) run() {}",
        ),
        ("Owner.kt", "class Owner { fun run() {} }"),
        ("Owner.scala", "class Owner { def run(): Unit = {} }"),
        ("owner.rb", "class Owner\n def run\n end\nend\n"),
        (
            "owner.php",
            "<?php class Owner { public function run() {} }",
        ),
        ("owner.swift", "class Owner { func run() {} }"),
    ] {
        let registration =
            CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", vec![], vec![]).unwrap();
        let mut build =
            SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
        parse_indexed_file(&mut build, path, source.as_bytes()).unwrap();
        let result = build.finish();
        let member = result.symbols.iter().find(|s| s.name == "run");
        let owner = member.and_then(|m| m.type_owner.as_ref());
        if !owner.is_some_and(|owner| {
            owner.relation == "direct_member"
                && result.symbols.iter().any(|s| {
                    s.type_owner.as_ref().is_some_and(|o| {
                        o.identity == owner.identity && o.relation == "declaration"
                    })
                })
        }) {
            failures.push(format!("{path}: {:?}", result.symbols));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn detached_implementations_keep_explicit_type_ownership() {
    let mut failures = Vec::new();
    for (path, source, relation) in [
        (
            "owner.cpp",
            "namespace demo { class Owner { public: void run(); }; void Owner::run() {} }",
            "direct_member",
        ),
        (
            "owner.rs",
            "struct Owner; trait Work { fn run(&self); } impl Work for Owner { fn run(&self) {} }",
            "trait_member",
        ),
        (
            "owner.go",
            "package demo\ntype Owner struct {}\nfunc (o *Owner) run() {}",
            "direct_member",
        ),
        (
            "owner.swift",
            "struct Owner {}\nextension Owner { func run() {} }",
            "direct_member",
        ),
        ("Owner.kt", "object Owner { fun run() {} }", "direct_member"),
        (
            "Owner.scala",
            "object Owner { def run(): Unit = {} }",
            "direct_member",
        ),
        (
            "owner.rb",
            "module Owner\n def self.run\n end\nend\n",
            "direct_member",
        ),
    ] {
        let registration =
            CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", vec![], vec![]).unwrap();
        let mut build =
            SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
        parse_indexed_file(&mut build, path, source.as_bytes()).unwrap();
        let result = build.finish();
        if !result.symbols.iter().any(|symbol| {
            symbol.name == "run"
                && symbol.type_owner.as_ref().is_some_and(|owner| {
                    owner.relation == relation
                        && result.symbols.iter().any(|s| {
                            s.type_owner.as_ref().is_some_and(|d| {
                                d.relation == "declaration" && d.identity == owner.identity
                            })
                        })
                })
        }) {
            failures.push(format!("{path}: {:?}", result.symbols));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn local_functions_and_nonmethod_languages_do_not_become_type_members() {
    for (path, source) in [
        (
            "owner.js",
            "class Owner { run = function () { function local() { target(); } }; }",
        ),
        (
            "owner.ts",
            "class Owner { run = function () { function local() { target(); } }; }",
        ),
        (
            "owner.py",
            "class Owner:\n    def run(self):\n        def local():\n            pass\n",
        ),
        ("owner.js", "class Owner { run() { function local() {} } }"),
        (
            "owner.rs",
            "struct Owner; impl Owner { fn run() { fn local() {} } }",
        ),
        ("owner.c", "struct Owner { int field; }; void local() {}"),
        ("owner.bzl", "def local():\n    pass\n"),
        ("owner.sh", "helper_local() { echo ok; }"),
    ] {
        let registration =
            CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", vec![], vec![]).unwrap();
        let mut build =
            SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
        parse_indexed_file(&mut build, path, source.as_bytes()).unwrap();
        let result = build.finish();
        let local = result
            .symbols
            .iter()
            .find(|s| {
                s.name
                    == if path.ends_with(".sh") {
                        "helper_local"
                    } else {
                        "local"
                    }
            })
            .unwrap_or_else(|| panic!("missing local in {path}: {:?}", result.symbols));
        assert!(local.type_owner.is_none(), "{path}: {local:?}");
    }
}

#[test]
fn type_ownership_visibility_ignores_comments() {
    for (path, source) in [
        ("owner.rs", "struct /* pub */ Owner;"),
        ("owner.swift", "struct /* public */ Owner {}"),
    ] {
        let snapshot = crate::code::syntax_snapshot_for_tests(&[(path, source)]);
        let owner = snapshot
            .symbols
            .iter()
            .find(|s| s.name == "Owner")
            .unwrap()
            .type_owner
            .as_ref()
            .unwrap();
        assert_eq!(
            owner.visibility.as_deref(),
            Some("restricted"),
            "{path}: {owner:?}"
        );
    }
}
