//! Full-parser coverage of eager execution and lexical binding boundaries.
use super::assert_python_kinds;
#[test]
fn python_execution_boundaries_preserve_runtime_identity() {
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def mutate():
 global overload
 overload = custom
mutate()
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def mutate():
 global overload
 overload = custom
class Container:
 mutate()
 @overload
 def pick(x: int): return leaf()
 def pick(x): return leaf()
result = Container.pick
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def factory():
 @overload
 def pick(x: int): return leaf()
 def pick(x): return leaf()
 overload = custom
 return pick
result = factory()
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def factory():
 @overload
 def pick(x: int): return leaf()
 def pick(x): return leaf()
 from typing import overload
 return pick
result = factory()
"#,
        false,
    );
    assert_python_kinds(
        r#"class Registry: pass
registry = Registry()
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
for registry.overload in [custom]:
 pass
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        true,
    );
    assert_python_kinds(
        r#"class Registry: pass
class Manager:
 def __enter__(self): return None
 def __exit__(self, *args): return False
registry = Registry()
manager = Manager()
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
with manager as registry.overload:
 pass
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        true,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def helper(value=(overload := custom)): pass
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
@(overload := custom)
def helper(): pass
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        false,
    );
}
#[test]
fn python_annotation_and_completion_boundaries_preserve_runtime_identity() {
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
class Helper((overload := custom, object)[1]): pass
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        false,
    );
    assert_python_kinds(
        r#"from __future__ import annotations
import typing
from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
marker: setattr(typing, "overload", custom)
@typing.overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        true,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
try:
 overload = custom
finally:
 from typing import overload
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        true,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
try:
 from typing import overload
finally:
 overload = custom
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def helper():
 global overload
 overload = custom
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        true,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
for overload in [custom]:
 pass
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        false,
    );
}

#[test]
fn python_comprehension_locals_do_not_hide_outer_walrus_writes() {
    for (expression, typed) in [
        ("[overload for overload in []]", true),
        ("[(overload := custom) for item in [1]]", false),
    ] {
        assert_python_kinds(
            &format!(
                "from typing import overload\ndef custom(fn): return fn\nsink={expression}\n@overload\ndef pick(x:int): ...\ndef pick(x): return x\n"
            ),
            typed,
        );
    }
}

#[test]
fn python_unproven_external_constructor_is_not_assumed_pure() {
    // The external constructor may be pure at runtime, but its body is outside
    // this proof. Keep it unknown instead of inventing a runtime purity claim.
    assert_python_kinds(
        "from types import SimpleNamespace\nfrom typing import overload\nregistry=SimpleNamespace()\nregistry.overload=custom\n@overload\ndef pick(x:int): ...\ndef pick(x): return x\n",
        false,
    );
}

#[test]
fn python_implicit_definition_effects_and_mutator_identity_are_not_assumed_pure() {
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
def alter(fn):
 global overload
 overload = custom
 return fn
@alter
def helper(): pass
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload, get_overloads
def custom(fn): return fn
def leaf(): return 1
class Helper:
 global overload
 overload = custom
@overload
def pick(x: int): return leaf()
def pick(x): return leaf()
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
class Helper:
 overload=custom
@overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
    assert_python_kinds(
        r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
class Helper:
 def alter(self):
  global overload
  overload=custom
@overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
    assert_python_kinds(
        r#"import typing
def custom(fn): return fn
def leaf(): return 1
def setattr(obj,key,value):
 obj.overload=custom
setattr(typing,"other",None)
@typing.overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        false,
    );
    assert_python_kinds(
        r#"import typing
def custom(fn): return fn
def leaf(): return 1
def delattr(obj,key):
 obj.overload=custom
delattr(typing,"other")
@typing.overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        false,
    );
}

#[test]
fn python_class_locals_and_parameter_annotation_reads_preserve_outer_bindings() {
    assert_python_kinds(
        r#"import typing
def leaf(): return 1
class Helper:
 typing=None
@typing.overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
    assert_python_kinds(
        r#"import typing
def leaf(): return 1
def helper(value: setattr): pass
setattr(typing,"other",None)
@typing.overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
}

#[test]
fn python_class_decorator_effects_require_the_actual_local_binding_proof() {
    assert_python_kinds(
        r#"import typing
def custom(fn): return fn
def alter(fn):
 typing.overload=custom
 return fn
def leaf(): return 1
class Fake:
 overload=alter
class Helper:
 typing=Fake
 @typing.overload
 def method(): pass
@typing.overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        false,
    );
    assert_python_kinds(
        r#"import typing
def leaf(): return 1
class Helper:
 @typing.overload
 def method(self,x:int): ...
 def method(self,x): return x
@typing.overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
}
