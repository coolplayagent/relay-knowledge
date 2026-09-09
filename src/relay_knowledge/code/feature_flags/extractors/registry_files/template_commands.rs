//! Bounded action tokenization for literal configuration calls in pipelines.
use crate::code::config_files::{
    template_actions::{Kind, Token, tokens},
    template_literals,
};

pub(super) struct Read {
    pub kind: &'static str,
    pub key: String,
    pub default: Option<String>,
    pub offset: usize,
}

pub(super) fn reads(action: &str) -> Vec<Read> {
    let tokens = tokens(action);
    let mut reads = Vec::new();
    let mut command_head = true;
    let mut template_name = false;
    let mut command_literal = None;
    let mut incoming = None;
    let mut parentheses = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Pipe => {
                incoming = command_literal.take();
                command_head = true;
            }
            Kind::Open => {
                parentheses.push((command_head, template_name, incoming));
                command_head = true;
                template_name = false;
                incoming = None;
                command_literal = None;
            }
            Kind::Close => {
                let result = command_literal;
                let Some((was_head, name, prior_input)) = parentheses.pop() else {
                    return Vec::new();
                };
                command_head = false;
                template_name = name;
                incoming = prior_input;
                command_literal = if was_head { result } else { None };
            }
            Kind::Assign => {
                incoming = None;
                command_literal = None;
                command_head = true;
            }
            Kind::Word("if" | "with" | "range" | "else") if command_head => {}
            Kind::Word("template" | "block") if command_head => template_name = true,
            Kind::Literal(_) if template_name => {
                template_name = false;
                command_head = true;
            }
            Kind::Word(function) if command_head => {
                command_head = false;
                command_literal = None;
                let kind = match function {
                    "key" | "keyOrDefault" => "config_key",
                    "env" => "env_var",
                    _ => continue,
                };
                let Some(literal) = literal_argument(tokens.get(index + 1), incoming) else {
                    continue;
                };
                let Some((key, _)) = template_literals::string(literal) else {
                    continue;
                };
                if !super::valid_key(&key) {
                    continue;
                }
                let default = if function == "keyOrDefault" {
                    literal_argument(tokens.get(index + 2), incoming).and_then(|literal| {
                        template_literals::string(literal).map(|(value, _)| value)
                    })
                } else {
                    None
                };
                reads.push(Read {
                    kind,
                    key,
                    default,
                    offset: token.offset,
                });
            }
            Kind::Literal(literal) if command_head => {
                command_literal = Some(literal);
                command_head = false;
            }
            Kind::Other | Kind::Comment => {}
            _ => {
                command_head = false;
                command_literal = None;
            }
        }
    }
    reads
}

// A pipeline appends its previous result as the last argument. Only a directly
// preceding literal command proves that value; intervening calls invalidate it.
fn literal_argument<'a>(token: Option<&Token<'a>>, incoming: Option<&'a str>) -> Option<&'a str> {
    match token.map(|token| token.kind) {
        Some(Kind::Literal(literal)) => Some(literal),
        None | Some(Kind::Close | Kind::Pipe) => incoming,
        _ => None,
    }
}

#[cfg(test)]
#[path = "template_commands_tests.rs"]
mod tests;
