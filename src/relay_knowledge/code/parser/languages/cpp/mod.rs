mod manual;
mod node_kinds;

pub(in crate::code::parser) use manual::{function_definition_is_destructor, manual_definitions};
pub(in crate::code::parser) use node_kinds::{definition_kind, is_call_node};
