use crate::{
    code::{SnapshotBuild, parser::parse_indexed_file},
    domain::CodeRepositoryRegistration,
};

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
