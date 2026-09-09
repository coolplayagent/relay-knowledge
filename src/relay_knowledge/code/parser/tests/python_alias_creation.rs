//! End-to-end parser coverage of module aliases and class creation.
use super::assert_python_kinds;
#[test]
fn python_module_aliases_and_class_creation_preserve_runtime_identity() {
    assert_python_kinds(
        r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
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
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
alias = typing
alias.unrelated = custom
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        true,
    );
    assert_python_kinds(
        r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Meta(type):
 def __new__(cls, name, bases, attrs):
  typing.overload = custom
  return type.__new__(cls, name, bases, attrs)
class Helper(metaclass=Meta): pass
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        false,
    );
    assert_python_kinds(
        r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Base:
 def __init_subclass__(cls):
  typing.overload = custom
class Helper(Base): pass
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        false,
    );
    assert_python_kinds(
        r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Descriptor:
 def __set_name__(self, owner, name):
  typing.overload = custom
descriptor = Descriptor()
import typing
class Helper:
 value = descriptor
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        false,
    );
    assert_python_kinds(
        r###"import typing
from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Helper:
 value = 1
 def method(self):
  typing.overload = custom
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"###,
        true,
    );

    assert_python_kinds(
        r###"import typing
def custom(fn): return fn
a = b = typing
b.overload = custom
def leaf(): return 1
@typing.overload
def pick(x: int): ...
def pick(x): return leaf()
"###,
        false,
    );

    assert_python_kinds(
        r###"import typing
class Target:
    from hooks import descriptor
def leaf(): return 1
@typing.overload
def pick(x: int): ...
def pick(x): return leaf()
"###,
        false,
    );
}
