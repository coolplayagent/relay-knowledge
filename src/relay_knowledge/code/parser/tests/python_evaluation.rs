//! Evaluation-time scope cases share the full-parser assertion with their owner.
use super::assert_python_kinds;

#[test]
fn python_eager_and_deferred_expression_scopes_match_binding_time() {
    // lambda_default
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=lambda value=(overload := custom): value
@overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        false,
    );
    // lambda_body
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=lambda:(overload := custom)
@overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
    // generator_body
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=((overload := custom) for item in [1])
@overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
    // generator_outer
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
import typing
sink=(item for item in (setattr(typing,"overload",custom),))
@typing.overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        false,
    );
    // list_body
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
sink=[(overload := custom) for item in [1]]
@overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        false,
    );
    // module_annotation
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
overload: object
@overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
    // class_annotation
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
class Box:
 from typing import overload
 overload: object
 @overload
 def pick(x:int): ...
 def pick(x): return leaf()
result=Box.pick
"#,
        true,
    );
    // annotation_value
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
overload: object=custom
@overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        false,
    );
    // factory_call
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
def factory():
 global overload
 @overload
 def pick(x:int): ...
 def pick(x): return leaf()
 return pick
result=factory()
overload=custom
"#,
        true,
    );
    // factory_later_import
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
def factory():
 global overload
 @overload
 def pick(x:int): ...
 def pick(x): return leaf()
 return pick
from typing import overload
result=factory()
overload=custom
"#,
        true,
    );
    // factory_prior_custom
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
from typing import overload
def factory():
 global overload
 @overload
 def pick(x:int): ...
 def pick(x): return leaf()
 return pick
overload=custom
result=factory()
"#,
        false,
    );
    // generator_module_body
    assert_python_kinds(
        r#"from typing import get_overloads
def custom(fn): return fn
def leaf(): return 1
import typing
sink=(setattr(typing,"overload",custom) for item in [1])
@typing.overload
def pick(x:int): ...
def pick(x): return leaf()
"#,
        true,
    );
}

#[test]
fn python_call_identity_and_local_annotation_do_not_invent_binding_proofs() {
    assert_python_kinds(
        r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
def factory():
 global overload
 @overload
 def pick(x:int): ...
 def pick(x): return leaf()
 return pick
original_factory=factory
factory=lambda: None
factory()
overload=custom
result=original_factory()
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
def factory(value):
 global overload
 @overload
 def pick(x:int): ...
 def pick(x): return leaf()
 return pick
result=factory((overload:=custom))
"#,
        false,
    );
    assert_python_kinds(
        r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
def mutate():
 global overload
 overload=custom
def factory():
 global overload
 @overload
 def pick(x:int): ...
 def pick(x): return leaf()
 return pick
mutate()
result=factory()
"#,
        false,
    );
}

#[test]
fn python_annotation_without_value_still_declares_a_function_local() {
    assert_python_kinds(
        r#"from typing import overload
def custom(fn): return fn
def leaf(): return 1
def factory():
 overload: object
 @overload
 def pick(x:int): ...
 def pick(x): return leaf()
 return pick
result=factory()
"#,
        false,
    );
}
