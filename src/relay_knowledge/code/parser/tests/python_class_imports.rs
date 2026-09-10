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

#[test]
fn eager_class_redirected_definitions_invalidate_outer_overload_proofs() {
    for (body, typed) in [
        (
            "nonlocal overload\n        def overload(fn): return fn",
            false,
        ),
        ("nonlocal overload\n        class overload: pass", false),
        ("def overload(fn): return fn", true),
        ("nonlocal other\n        def other(fn): return fn", true),
        ("global overload\n        def overload(fn): return fn", true),
        (
            "nonlocal overload\n        def unrelated(fn): return fn",
            true,
        ),
        (
            "def later():\n            nonlocal overload\n            overload = lambda fn: fn",
            true,
        ),
    ] {
        assert_python_kinds(
            &format!(
                "def outer():\n    from typing import overload\n    other = None\n    class Change:\n        {body}\n    @overload\n    def pick(): return leaf()\n    def pick(): return final_leaf()\n"
            ),
            typed,
        );
    }
    assert_python_kinds(
        "from typing import overload\ndef outer():\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def pick(): return leaf()\n    def pick(): return final_leaf()\n",
        false,
    );
    assert_python_kinds(
        "def outer():\n    from typing import overload as ov\n    class Change:\n        nonlocal ov\n        def ov(fn): return fn\n    @ov\n    def pick(): return leaf()\n    def pick(): return final_leaf()\n",
        false,
    );
}
