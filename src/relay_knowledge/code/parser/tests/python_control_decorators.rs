//! Definition-time expression order and conditional module alias evidence.
use super::assert_python_kinds;
#[test]
fn python_control_aliases_and_sibling_decorators_preserve_runtime_identity() {
    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
def leaf(): return 1
if True:
 alias = typing
alias.overload = custom
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        false,
    );
    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
def leaf(): return 1
if True:
 alias = typing
alias.other = custom
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        true,
    );
    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
def leaf(): return 1
if True:
 if True:
  alias = typing
alias.overload = custom
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        false,
    );
    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
def leaf(): return 1
def make_mutator():
 typing.overload = custom
 return custom
@make_mutator()
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        false,
    );
    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
def leaf(): return 1
def make_mutator():
 typing.overload = custom
 return custom
@typing.overload
@make_mutator()
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        true,
    );
    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
def leaf(): return 1
@custom
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        true,
    );
    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
def leaf(): return 1
def make_mutator():
 typing.overload = custom
 return custom
@make_mutator()
# intervening comment
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        false,
    );
    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
def leaf(): return 1
def make_mutator():
 typing.overload = custom
 return custom
@typing.overload
# intervening comment
@make_mutator()
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        true,
    );
}
