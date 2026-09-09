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

#[test]
fn python_later_imports_respect_linear_binding_order_and_execution_boundaries() {
    assert_python_kinds(
        r#"def outer():
 def inner():
  @ov
  def pick(x:int): ...
  def pick(x): return x
  return pick
 from typing import overload as ov
 return inner
result=outer()()
"#,
        true,
    );
    assert_python_kinds(
        r#"def outer():
 def inner():
  @ov
  def pick(x:int): ...
  def pick(x): return x
  return pick
 ov=lambda fn: fn
 from typing import overload as ov
 return inner
result=outer()()
"#,
        true,
    );
    assert_python_kinds(
        r#"def outer():
 def inner():
  @ov
  def pick(x:int): ...
  def pick(x): return x
  return pick
 from typing import overload as ov
 ov=lambda fn: fn
 return inner
result=outer()()
"#,
        false,
    );
    assert_python_kinds(
        r#"def outer():
 def inner():
  @ov
  def pick(x:int): ...
  def pick(x): return x
  return pick
 result=inner()
 from typing import overload as ov
 return result
result=outer()
"#,
        false,
    );
    assert_python_kinds(
        r#"def outer():
 def inner():
  @ov
  def pick(x:int): ...
  def pick(x): return x
  return pick
 return inner
 from typing import overload as ov
result=outer()()
"#,
        false,
    );
}

#[test]
fn python_module_mutator_calls_and_duplicate_import_aliases_preserve_the_last_binding() {
    for (prefix, decorator, typed) in [
        (
            "import typing\nsetattr(typing, \"overload\", custom)\n",
            "typing.overload",
            false,
        ),
        (
            "import typing\nsetattr(typing, \"other\", custom)\n",
            "typing.overload",
            true,
        ),
        (
            "import typing\ndelattr(typing, \"overload\")\n",
            "typing.overload",
            false,
        ),
        (
            "import typing\nsetattr(other, \"typing\", custom)\n",
            "typing.overload",
            true,
        ),
        ("import typing as tm, types as tm\n", "tm.overload", false),
        ("import types as tm, typing as tm\n", "tm.overload", true),
        (
            "from typing import overload as ov, no_type_check as ov\n",
            "ov",
            false,
        ),
        (
            "from typing import no_type_check as ov, overload as ov\n",
            "ov",
            true,
        ),
    ] {
        assert_python_kinds(
            &format!("{prefix}@{decorator}\ndef pick(x:int): ...\ndef pick(x): return x\n"),
            typed,
        );
    }
}

#[test]
fn python_chained_targets_and_parenthesized_decorators_keep_binding_identity() {
    for (prefix, decorator, typed) in [
        (
            "from typing import overload\nother=overload=custom\n",
            "overload",
            false,
        ),
        (
            "import typing\nother=typing.overload=custom\n",
            "typing.overload",
            false,
        ),
        (
            "from typing import overload\nother=(sink,overload)=(custom,custom)\n",
            "overload",
            false,
        ),
        (
            "from typing import overload\nfirst=second=overload\n",
            "overload",
            true,
        ),
        (
            "import typing\nfirst=typing.other=custom\n",
            "typing.overload",
            true,
        ),
        ("from typing import overload\n", "((overload))", true),
        ("import typing\n", "((typing.overload))", true),
        ("import typing\n", "((typing)).overload", true),
        ("overload=custom\n", "(overload)", false),
    ] {
        assert_python_kinds(
            &format!("{prefix}@{decorator}\ndef pick(x:int): ...\ndef pick(x): return x\n"),
            typed,
        );
    }
}

#[test]
fn python_decorator_comments_and_boolean_reads_are_not_binding_changes() {
    assert_python_kinds(
        "from typing import overload\noverload and True\n@(\n # comment\n overload\n)\ndef pick(x:int): ...\ndef pick(x): return x\n",
        true,
    );
    assert_python_kinds(
        "import typing\n@((\n # receiver\n typing\n)).overload\ndef pick(x:int): ...\ndef pick(x): return x\n",
        true,
    );
}

#[test]
fn python_immediate_expression_writes_and_comment_only_gaps_preserve_bindings() {
    for (prefix, decorator, typed) in [
        (
            "from typing import overload\n(overload := custom)\n",
            "overload",
            false,
        ),
        (
            "from typing import overload\nsink=(0,(overload := custom))\n",
            "overload",
            false,
        ),
        (
            "from typing import overload\nTrue and (overload := custom)\n",
            "overload",
            false,
        ),
        (
            "from typing import overload\n(other := overload)\n",
            "overload",
            true,
        ),
        (
            "from typing import overload\nsink=lambda:(overload := custom)\n",
            "overload",
            true,
        ),
        (
            "import typing\n((typing)).overload=custom\n",
            "typing.overload",
            false,
        ),
        (
            "import typing\n((typing)).__dict__['overload']=custom\n",
            "typing.overload",
            false,
        ),
        (
            "import typing\nsetattr((typing), 'overload', custom)\n",
            "typing.overload",
            false,
        ),
        (
            "import typing\n((typing)).other=custom\n",
            "typing.overload",
            true,
        ),
        (
            "import typing\n((typing)).__dict__['other']=custom\n",
            "typing.overload",
            true,
        ),
    ] {
        assert_python_kinds(
            &format!("{prefix}@{decorator}\ndef pick(x:int): ...\ndef pick(x): return x\n"),
            typed,
        );
    }
    assert_python_kinds(
        "def factory():\n global overload\n @overload\n def pick(x:int): ...\n def pick(x): return x\n return pick\n# no operation\nfrom typing import overload\nresult=factory()\n",
        true,
    );
}

#[path = "python_evaluation.rs"]
mod evaluation;

#[path = "python_boundaries.rs"]
mod boundaries;
