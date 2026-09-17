use super::*;

#[test]
fn java_implicit_base_and_synthesized_members_do_not_bind_to_unrelated_overloads() {
    for (source, hint) in [
        (
            "class A {boolean equals(int n){return true;} boolean run(){return this.equals(\"x\");}}",
            "this.equals",
        ),
        (
            "enum E {A; int compareTo(int n){return n;} int run(){return this.compareTo(A);}}",
            "this.compareTo",
        ),
        (
            "record R(int value) {int value(int n){return n;} int run(){return this.value();}}",
            "this.value",
        ),
        (
            "enum E {A {String run(){return this.toString();}}, B {public String toString(){return \"B\";}};}",
            "this.toString",
        ),
    ] {
        let snapshot = parse_sources(&[("A.java", source)]);
        assert_eq!(
            snapshot.files[0].parse_status,
            CodeParseStatus::Parsed,
            "{source}"
        );
        let calls: Vec<_> = snapshot
            .calls
            .iter()
            .filter(|call| call.target_hint.as_deref() == Some(hint))
            .collect();
        assert!(!calls.is_empty(), "{source}: {:?}", snapshot.calls);
        assert!(
            calls
                .iter()
                .all(|call| call.resolution_state == "unresolved"),
            "{source}: {calls:?}"
        );
        if source.contains("String run") {
            assert!(
                snapshot
                    .symbols
                    .iter()
                    .filter(|symbol| matches!(symbol.name.as_str(), "run" | "toString"))
                    .all(|symbol| symbol.type_owner.is_none()),
                "{:?}",
                snapshot.symbols
            );
        }
    }
}

#[test]
fn java_this_does_not_select_a_direct_member_over_an_inherited_overload() {
    for source in [
        "class Base {void process(String s){}} class B extends Base {void process(int n){} void run(){this.process(\"x\");}}",
        "class Base {static void process(String s){}} class B extends Base {static void process(int n){} void run(){this.process(\"x\");}}",
        "class A {void process(){} Object x=new External(){void run(){this.process();}};}",
    ] {
        let snapshot = parse_sources(&[("A.java", source)]);
        let calls: Vec<_> = snapshot
            .calls
            .iter()
            .filter(|call| call.target_hint.as_deref() == Some("this.process"))
            .collect();
        assert!(!calls.is_empty(), "{source}: {:?}", snapshot.calls);
        assert!(
            calls
                .iter()
                .all(|call| call.resolution_state == "unresolved"),
            "{source}: {calls:?}"
        );
        if source.contains("new External") {
            assert!(
                snapshot
                    .symbols
                    .iter()
                    .find(|s| s.name == "run")
                    .unwrap()
                    .type_owner
                    .is_none()
            );
        }
    }
}

#[test]
fn unknown_java_type_candidates_keep_the_source_receiver_hint() {
    let snapshot = parse_sources(&[(
        "A.java",
        "package demo; class A {void run(){ Missing.process(); }}",
    )]);
    let call = snapshot.calls.first().unwrap();
    assert_eq!(call.resolution_state, "unresolved");
    assert_eq!(call.target_hint.as_deref(), Some("Missing.process"));
    assert_eq!(call.callee_name, "Missing.process");
}

#[test]
fn java_pattern_type_names_remain_distinct_from_their_variable_bindings() {
    for caller in [
        "package demo; class A {void run(Object v){ switch(v){case Client value -> Client.process(); default -> {}} }}",
        "package demo; class A {void run(Object v){ if(v instanceof Box(Client value)) Client.process(); }}",
    ] {
        let snapshot = parse_sources(&[
            ("A.java", caller),
            (
                "Client.java",
                "package demo; class Client {static void process(){}}",
            ),
        ]);
        assert_eq!(
            snapshot
                .files
                .iter()
                .find(|f| f.path == "A.java")
                .unwrap()
                .parse_status,
            CodeParseStatus::Parsed
        );
        let call = snapshot
            .calls
            .iter()
            .find(|call| call.path == "A.java")
            .unwrap();
        assert_eq!(call.resolution_state, "resolved", "{caller}: {call:?}");
    }
}

#[test]
fn java_method_and_constructor_names_do_not_shadow_type_receivers() {
    for caller in [
        "package demo; class B { B(){} static void process(){} void run(){B.process();} }",
        "package demo; class A {void B(){} void run(){B.process();}} class B {static void process(){}}",
    ] {
        let snapshot = parse_sources(&[("A.java", caller)]);
        let calls: Vec<_> = snapshot
            .calls
            .iter()
            .filter(|call| call.target_hint.as_deref() == Some("demo.B.process"))
            .collect();
        assert!(!calls.is_empty(), "{caller}: {:?}", snapshot.calls);
        assert!(
            calls.iter().all(|call| call.resolution_state == "resolved"),
            "{caller}: {calls:?}"
        );
    }
}

#[test]
fn java_switch_and_record_patterns_are_real_receiver_bindings() {
    for caller in [
        "package demo; class A {void run(Object v){ switch(v){case Client B -> B.process(); default -> {}} }}",
        "package demo; class A {void run(Object v){ if(v instanceof Box(Client B)) B.process(); }}",
    ] {
        let snapshot = parse_sources(&[
            ("A.java", caller),
            ("B.java", "package demo; class B {static void process(){}}"),
        ]);
        assert_eq!(
            snapshot
                .files
                .iter()
                .find(|f| f.path == "A.java")
                .unwrap()
                .parse_status,
            CodeParseStatus::Parsed,
            "{caller}"
        );
        let calls: Vec<_> = snapshot
            .calls
            .iter()
            .filter(|call| call.path == "A.java")
            .collect();
        assert!(!calls.is_empty());
        assert!(
            calls.iter().all(|call| call.resolution_state != "resolved"),
            "{caller}: {calls:?}"
        );
    }
}

#[test]
fn java_static_calls_use_package_or_explicit_import_and_ast_modifiers() {
    for caller in [
        "package demo; class A { void run() { B.process(); } }",
        "package other; import demo.B; class A { void run() { B.process(); } }",
        "package other; import demo.B; import demo.B; class A { void run() { B.process(); } }",
    ] {
        let snapshot = parse_sources(&[
            ("misplaced/A.java", caller),
            (
                "different/B.java",
                "package demo; public class B { public static void process() {} }",
            ),
            ("other.js", "class B { static process() {} }"),
        ]);
        let call = snapshot
            .calls
            .iter()
            .find(|c| c.path == "misplaced/A.java")
            .unwrap();
        assert_eq!(call.resolution_state, "resolved", "{call:?}");
        assert_eq!(call.target_hint.as_deref(), Some("demo.B.process"));
        let target = snapshot
            .symbols
            .iter()
            .find(|s| Some(&s.symbol_snapshot_id) == call.callee_symbol_snapshot_id.as_ref())
            .unwrap();
        assert_eq!(target.path, "different/B.java");
        assert_eq!(
            target.type_owner.as_ref().unwrap().static_dispatch,
            Some(true)
        );
    }
}

#[test]
fn java_type_receivers_reject_shadowing_and_unknown_inherited_bindings() {
    for caller in [
        "package demo; class A { Object B; void run(){ B.process(); } }",
        "package demo; class A { void run(Object B){ B.process(); } }",
        "package demo; class A { <B> void run(){ B.process(); } }",
        "package demo; class A { void run(){ Object B=null; B.process(); } }",
        "package demo; class A { void run(){ for(Object B:items) B.process(); } }",
        "package demo; class A { void run(){ try {} catch(Exception B) { B.process(); } } }",
        "package demo; class A { void run(){ try(Resource B=open()){B.process();} } }",
        "package demo; class A { void run(){ items.forEach(B -> B.process()); } }",
        "package demo; class A { void run(){ items.forEach((B) -> B.process()); } }",
        "package demo; class A { void run(Object value){ if(value instanceof Client B) B.process(); } }",
        "package demo; record A(Object B) { void run(){ B.process(); } }",
        "package demo; enum A { B; void run(){ B.process(); } }",
        "package demo; class A { class B {} void run(){ B.process(); } }",
        "package demo; class A extends Base { void run(){ B.process(); } }",
        "package demo; class A extends Base { class Inner { void run(){ B.process(); } } }",
        "package demo; class A { Object x = new Base(){ void run(){ B.process(); } }; }",
        "package other; import demo.B; import static elsewhere.Names.B; class A {void run(){B.process();}}",
        "package other; import demo.B; import static elsewhere.Names.*; class A {void run(){B.process();}}",
    ] {
        let snapshot = parse_sources(&[
            ("A.java", caller),
            (
                "B.java",
                "package demo; public class B { public static void process() {} }",
            ),
        ]);
        let calls: Vec<_> = snapshot
            .calls
            .iter()
            .filter(|c| {
                c.path == "A.java"
                    && c.target_hint
                        .as_deref()
                        .is_some_and(|hint| hint.ends_with("B.process"))
            })
            .collect();
        assert!(!calls.is_empty(), "{caller}: {:?}", snapshot.calls);
        assert!(
            calls.iter().all(|call| call.resolution_state != "resolved"),
            "{caller}: {calls:?}"
        );
    }
}

#[test]
fn java_static_calls_do_not_infer_modifiers_or_inherited_overload_choice() {
    for target in [
        "package demo; class B { @Note(\"static\") public void process() {} }",
        "package demo; class B { /*static*/ public void process() {} }",
        "package demo; class B { public void process() { String x=\"static\"; } }",
        "package demo; class B extends Base { public static void process(int n) {} }",
        "package demo; interface B extends Base { static void process(int n) {} }",
    ] {
        let snapshot = parse_sources(&[
            (
                "A.java",
                "package demo; class A {void run(){ B.process(\"x\"); }}",
            ),
            ("B.java", target),
        ]);
        assert!(
            snapshot
                .calls
                .iter()
                .filter(|call| call.path == "A.java")
                .all(|call| call.resolution_state != "resolved"),
            "{target}: {:?}",
            snapshot.calls
        );
    }
}

#[test]
fn duplicate_java_static_targets_stay_ambiguous_without_path_preference() {
    let snapshot = parse_sources(&[
        (
            "A.java",
            "package demo; class A {void run(){ B.process(); }} class B {public static void process(){}}",
        ),
        (
            "B.java",
            "package demo; class B {public static void process(){}}",
        ),
    ]);
    let call = snapshot
        .calls
        .iter()
        .find(|call| call.path == "A.java")
        .unwrap();
    assert_eq!(call.resolution_state, "ambiguous", "{call:?}");
}
