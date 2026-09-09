//! Bounded action tokenization for literal configuration calls in pipelines.
use super::template_literals;

pub(super) struct Read {
    pub kind: &'static str,
    pub key: String,
    pub default: Option<String>,
    pub offset: usize,
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Word(&'a str),
    Literal(&'a str),
    Open,
    Close,
    Pipe,
    Assign,
    Other,
}

struct Token<'a> {
    kind: Kind<'a>,
    offset: usize,
}

pub(super) fn reads(action: &str) -> Vec<Read> {
    let tokens = tokens(action);
    let mut reads = Vec::new();
    let mut command_head = true;
    let mut template_name = false;
    let mut command_literal = None;
    let mut incoming = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Pipe => {
                incoming = command_literal.take();
                command_head = true;
            }
            Kind::Open | Kind::Assign | Kind::Close => {
                incoming = None;
                command_literal = None;
                command_head = !matches!(token.kind, Kind::Close);
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
            Kind::Other => {}
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

// The caller enforces the 64 KiB action bound; token slices borrow that action.
fn tokens(action: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let bytes = action.as_bytes();
    let mut offset = 0;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte.is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        if bytes[offset..].starts_with(b"/*") {
            let Some(end) = action[offset + 2..].find("*/") else {
                break;
            };
            offset += end + 4;
            continue;
        }
        let start = offset;
        offset += 1;
        let kind = match byte {
            b'(' => Kind::Open,
            b')' => Kind::Close,
            b'|' => Kind::Pipe,
            b'=' => Kind::Assign,
            b':' if bytes.get(offset) == Some(&b'=') => {
                offset += 1;
                Kind::Assign
            }
            b',' | b'-' => Kind::Other,
            b'"' | b'`' | b'\'' => {
                while offset < bytes.len() {
                    let next = bytes[offset];
                    offset += 1;
                    if next == b'\\' && byte != b'`' {
                        offset = (offset + 1).min(bytes.len());
                    } else if next == byte {
                        break;
                    }
                }
                Kind::Literal(&action[start..offset])
            }
            _ => {
                while offset < bytes.len()
                    && !bytes[offset].is_ascii_whitespace()
                    && !matches!(
                        bytes[offset],
                        b'(' | b')' | b'|' | b'=' | b':' | b',' | b'"' | b'`' | b'\''
                    )
                {
                    offset += 1;
                }
                Kind::Word(&action[start..offset])
            }
        };
        tokens.push(Token {
            kind,
            offset: start,
        });
    }
    tokens
}

#[cfg(test)]
#[path = "template_commands_tests.rs"]
mod tests;
