//! Whole-parser coverage for imports executed during class construction.
use super::assert_python_kinds;

#[test]
fn class_module_imports_require_proven_standard_origins() {
    for (body, typed) in [
        ("import side_effect", false),
        ("import typing", true),
        ("import typing as provider", true),
        ("import typing, side_effect", false),
        ("value = 1", true),
    ] {
        assert_python_kinds(
            &format!(
                "from typing import overload\nclass Helper:\n {body}\n@overload\ndef pick(x:int): ...\ndef pick(x): return x\n"
            ),
            typed,
        );
    }
}
