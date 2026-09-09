//! Full parser proofs for import fallback paths and exact module mutation targets.
use super::*;
#[test]
fn python_import_fallback_requires_every_branch_to_prove_the_typing_binding() {
    let cases = [
        (
            "try:\n from typing import overload as ov\nexcept ImportError:\n from typing_extensions import overload as ov\n",
            "ov",
            true,
        ),
        (
            "try:\n import typing as tm\nexcept ImportError:\n import typing_extensions as tm\n",
            "tm.overload",
            true,
        ),
        (
            "try:\n from typing import overload as ov\nexcept ImportError:\n from custom import overload as ov\n",
            "ov",
            false,
        ),
        (
            "try:\n from typing import overload as ov\nexcept ImportError:\n from typing_extensions import overload as ov\nfinally:\n ov=custom\n",
            "ov",
            false,
        ),
        (
            "try:\n from typing import overload as ov\nexcept ImportError as ov:\n from typing_extensions import overload as ov\n",
            "ov",
            false,
        ),
        (
            "try:\n from typing import overload as ov\nexcept ImportError:\n from typing_extensions import overload as ov\nelse:\n ov=custom\n",
            "ov",
            false,
        ),
    ];
    for (prefix, decorator, declaration) in cases {
        assert_python_kinds(
            &format!(
                "{prefix}@{decorator}\ndef pick(value:int): ...\ndef pick(value): return value\n"
            ),
            declaration,
        );
    }
    let exhausted = format!(
        "try:\n{} from typing import overload as ov\nexcept ImportError:\n from typing_extensions import overload as ov\n@ov\ndef pick(value:int): ...\ndef pick(value): return value\n",
        " pass\n".repeat(1025)
    );
    assert_python_kinds(&exhausted, false);
}
#[test]
fn python_module_members_only_invalidate_the_overload_value_or_unknown_namespace_key() {
    for (target, declaration) in [
        ("typing.other=custom", true),
        ("typing.__dict__[\"other\"]=custom", true),
        ("typing.overload=custom", false),
        ("typing.__dict__[\"overload\"]=custom", false),
        ("typing.__dict__[key]=custom", false),
    ] {
        assert_python_kinds(
            &format!(
                "import typing\n{target}\n@typing.overload\ndef pick(value:int): ...\ndef pick(value): return value\n"
            ),
            declaration,
        );
    }
}
fn assert_python_kinds(source: &str, declaration: bool) {
    let registration = crate::domain::CodeRepositoryRegistration::new(
        "repo",
        "alias",
        "/tmp/repo",
        vec![],
        vec![],
    )
    .unwrap();
    let mut build = SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
    parse_indexed_file(&mut build, "sample.py", source.as_bytes()).unwrap();
    let snapshot = build.finish();
    let kinds = snapshot
        .symbols
        .iter()
        .filter(|s| s.name == "pick")
        .map(|s| s.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            if declaration {
                "function_declaration"
            } else {
                "function"
            },
            "function"
        ],
        "{source}"
    );
}
