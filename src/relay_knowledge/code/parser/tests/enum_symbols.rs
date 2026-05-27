use crate::domain::CodeRepositoryRegistration;

use super::*;

#[test]
fn rust_enum_members_are_indexed_under_enum_owner_identities() {
    let unicode_enum = "\u{72b6}\u{6001}";
    let unicode_variant = "\u{5df2}\u{5b8c}\u{6210}";
    let source = format!(
        r#"
enum Color {{
    Red,
    Blue,
}}

enum State {{
    Red,
}}

enum Keyword {{
    r#type,
}}

enum {unicode_enum} {{
    {unicode_variant},
}}
"#
    );
    let snapshot = parse_source_snapshot("src/types.rs", source.as_bytes());

    let color = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Color")
        .expect("enum type should be indexed");
    let red_variants = snapshot
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "Red")
        .collect::<Vec<_>>();
    let blue = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Blue")
        .expect("enum member should be indexed");

    assert_eq!(color.kind, "class");
    assert_eq!(red_variants.len(), 2);
    assert!(
        red_variants
            .iter()
            .all(|symbol| symbol.kind == "enum_member")
    );
    assert_eq!(blue.kind, "enum_member");
    assert!(
        red_variants
            .iter()
            .any(|symbol| symbol.canonical_symbol_id.contains("Color.Red")),
        "Color::Red should keep the enum owner identity: {:?}",
        red_variants
    );
    assert!(
        red_variants
            .iter()
            .any(|symbol| symbol.canonical_symbol_id.contains("State.Red")),
        "State::Red should remain distinguishable from Color::Red: {:?}",
        red_variants
    );
    assert!(blue.canonical_symbol_id.contains("Color.Blue"));
    let raw_keyword = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "r#type")
        .expect("raw identifier enum member should be indexed");

    assert_eq!(raw_keyword.kind, "enum_member");
    assert!(
        raw_keyword.canonical_symbol_id.contains("Keyword.r#type"),
        "raw identifier enum member should keep its owner identity: {raw_keyword:?}"
    );
    let unicode_member = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == unicode_variant)
        .expect("Unicode enum member should be indexed");

    assert_eq!(unicode_member.kind, "enum_member");
    assert!(
        unicode_member
            .canonical_symbol_id
            .contains(&format!("{unicode_enum}.{unicode_variant}")),
        "Unicode enum member should keep its owner identity: {unicode_member:?}"
    );
}

#[test]
fn c_enum_members_are_indexed_under_enum_owner_identities() {
    let snapshot = parse_source_snapshot(
        "include/direction.h",
        br#"
enum Direction {
    kForward,
    kReverse,
};
"#,
    );
    let direction = snapshot
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Direction")
        .expect("C enum tag should be indexed as a type symbol");

    assert_eq!(direction.kind, "type");
    assert_enum_members_owned_by(&snapshot, "Direction", ["kForward", "kReverse"]);
}

#[test]
fn cpp_inline_enum_members_are_indexed_under_enum_owner_identities() {
    let snapshot = parse_source_snapshot(
        "db/db_iter.cc",
        br#"
class DBIter {
 public:
  enum Direction { kForward, kReverse };
};
"#,
    );

    assert_enum_members_owned_by(&snapshot, "Direction", ["kForward", "kReverse"]);
}

fn assert_enum_members_owned_by<const N: usize>(
    snapshot: &crate::domain::CodeIndexSnapshot,
    owner: &str,
    names: [&str; N],
) {
    for name in names {
        let member = snapshot
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("{name} should be indexed as an enum member"));
        assert_eq!(member.kind, "enum_member");
        assert!(
            member
                .canonical_symbol_id
                .contains(&format!("{owner}.{name}")),
            "enum member should be owned by {owner}: {member:?}"
        );
    }
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
