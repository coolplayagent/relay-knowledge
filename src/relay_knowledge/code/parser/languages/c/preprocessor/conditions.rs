//! Bounded C preprocessor-condition tokenization and expression evaluation.

use std::collections::{HashMap, HashSet};

use super::super::lexical::c_identifier_char;
use super::ActiveMacroDefinition;

pub(super) fn evaluate_if_condition(
    expression: &str,
    active_macros: &HashMap<String, ActiveMacroDefinition>,
) -> bool {
    let mut visiting_macros = HashSet::new();
    evaluate_if_condition_value(expression, active_macros, &mut visiting_macros)
        .is_some_and(|value| value != 0)
}

fn evaluate_if_condition_value(
    expression: &str,
    active_macros: &HashMap<String, ActiveMacroDefinition>,
    visiting_macros: &mut HashSet<String>,
) -> Option<i128> {
    let expression = strip_c_comments(expression)?;
    let tokens = tokenize_condition_expression(&expression)?;
    if tokens.is_empty() {
        return None;
    }
    let mut parser = PreprocessorConditionParser {
        tokens: &tokens,
        active_macros,
        visiting_macros,
        position: 0,
    };
    let value = parser.parse_expression()?;
    if parser.finished() { Some(value) } else { None }
}

#[derive(Clone, Copy)]
enum ConditionToken<'a> {
    Number(&'a str),
    Identifier(&'a str),
    Defined,
    Bang,
    AndAnd,
    OrOr,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    LeftParen,
    RightParen,
}

struct PreprocessorConditionParser<'tokens, 'macros, 'visiting> {
    tokens: &'tokens [ConditionToken<'tokens>],
    active_macros: &'macros HashMap<String, ActiveMacroDefinition>,
    visiting_macros: &'visiting mut HashSet<String>,
    position: usize,
}

impl<'tokens, 'macros, 'visiting> PreprocessorConditionParser<'tokens, 'macros, 'visiting> {
    fn parse_expression(&mut self) -> Option<i128> {
        self.parse_logical_or()
    }

    fn parse_logical_or(&mut self) -> Option<i128> {
        let mut value = self.parse_logical_and()?;
        while matches!(self.peek(), Some(ConditionToken::OrOr)) {
            self.position += 1;
            let right = self.parse_logical_and()?;
            value = bool_value(value != 0 || right != 0);
        }
        Some(value)
    }

    fn parse_logical_and(&mut self) -> Option<i128> {
        let mut value = self.parse_comparison()?;
        while matches!(self.peek(), Some(ConditionToken::AndAnd)) {
            self.position += 1;
            let right = self.parse_comparison()?;
            value = bool_value(value != 0 && right != 0);
        }
        Some(value)
    }

    fn parse_comparison(&mut self) -> Option<i128> {
        let mut value = self.parse_unary()?;
        loop {
            let comparison = match self.peek() {
                Some(ConditionToken::EqualEqual) => |left, right| left == right,
                Some(ConditionToken::BangEqual) => |left, right| left != right,
                Some(ConditionToken::Less) => |left, right| left < right,
                Some(ConditionToken::LessEqual) => |left, right| left <= right,
                Some(ConditionToken::Greater) => |left, right| left > right,
                Some(ConditionToken::GreaterEqual) => |left, right| left >= right,
                _ => break,
            };
            self.position += 1;
            let right = self.parse_unary()?;
            value = bool_value(comparison(value, right));
        }
        Some(value)
    }

    fn parse_unary(&mut self) -> Option<i128> {
        if matches!(self.peek(), Some(ConditionToken::Bang)) {
            self.position += 1;
            return Some(bool_value(self.parse_unary()? == 0));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Option<i128> {
        match self.peek()? {
            ConditionToken::Number(literal) => {
                self.position += 1;
                parse_integer_literal(literal)
            }
            ConditionToken::Identifier(name) => {
                self.position += 1;
                macro_condition_value(name, self.active_macros, self.visiting_macros)
            }
            ConditionToken::Defined => {
                self.position += 1;
                self.parse_defined_expression()
            }
            ConditionToken::LeftParen => {
                self.position += 1;
                let value = self.parse_expression()?;
                if matches!(self.peek(), Some(ConditionToken::RightParen)) {
                    self.position += 1;
                    Some(value)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn parse_defined_expression(&mut self) -> Option<i128> {
        match self.peek()? {
            ConditionToken::Identifier(name) => {
                self.position += 1;
                Some(bool_value(self.active_macros.contains_key(name)))
            }
            ConditionToken::LeftParen => {
                self.position += 1;
                let name = match self.peek()? {
                    ConditionToken::Identifier(name) => {
                        self.position += 1;
                        name
                    }
                    _ => return None,
                };
                if !matches!(self.peek(), Some(ConditionToken::RightParen)) {
                    return None;
                }
                self.position += 1;
                Some(bool_value(self.active_macros.contains_key(name)))
            }
            _ => None,
        }
    }

    fn peek(&self) -> Option<ConditionToken<'tokens>> {
        self.tokens.get(self.position).copied()
    }

    fn finished(&self) -> bool {
        self.position == self.tokens.len()
    }
}

fn macro_condition_value(
    name: &str,
    active_macros: &HashMap<String, ActiveMacroDefinition>,
    visiting_macros: &mut HashSet<String>,
) -> Option<i128> {
    let Some(definition) = active_macros.get(name) else {
        return Some(0);
    };
    if definition.function_like {
        return Some(0);
    }
    let replacement = definition.replacement.trim();
    if replacement.is_empty() {
        return Some(0);
    }
    if !visiting_macros.insert(name.to_owned()) {
        return None;
    }
    let value = evaluate_if_condition_value(replacement, active_macros, visiting_macros);
    visiting_macros.remove(name);
    value
}

fn bool_value(value: bool) -> i128 {
    i128::from(value)
}

fn tokenize_condition_expression(expression: &str) -> Option<Vec<ConditionToken<'_>>> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < expression.len() {
        let rest = &expression[index..];
        let Some(character) = rest.chars().next() else {
            break;
        };
        if character.is_whitespace() {
            index += character.len_utf8();
            continue;
        }
        if rest.starts_with("&&") {
            tokens.push(ConditionToken::AndAnd);
            index += 2;
            continue;
        }
        if rest.starts_with("||") {
            tokens.push(ConditionToken::OrOr);
            index += 2;
            continue;
        }
        if rest.starts_with("==") {
            tokens.push(ConditionToken::EqualEqual);
            index += 2;
            continue;
        }
        if rest.starts_with("!=") {
            tokens.push(ConditionToken::BangEqual);
            index += 2;
            continue;
        }
        if rest.starts_with("<=") {
            tokens.push(ConditionToken::LessEqual);
            index += 2;
            continue;
        }
        if rest.starts_with(">=") {
            tokens.push(ConditionToken::GreaterEqual);
            index += 2;
            continue;
        }
        match character {
            '!' => {
                tokens.push(ConditionToken::Bang);
                index += 1;
            }
            '<' => {
                tokens.push(ConditionToken::Less);
                index += 1;
            }
            '>' => {
                tokens.push(ConditionToken::Greater);
                index += 1;
            }
            '(' => {
                tokens.push(ConditionToken::LeftParen);
                index += 1;
            }
            ')' => {
                tokens.push(ConditionToken::RightParen);
                index += 1;
            }
            '0'..='9' => {
                let end = scan_condition_number(expression, index);
                tokens.push(ConditionToken::Number(&expression[index..end]));
                index = end;
            }
            _ if c_identifier_start(character) => {
                let end = scan_condition_identifier(expression, index);
                let name = &expression[index..end];
                if name == "defined" {
                    tokens.push(ConditionToken::Defined);
                } else {
                    tokens.push(ConditionToken::Identifier(name));
                }
                index = end;
            }
            _ => return None,
        }
    }
    Some(tokens)
}

fn scan_condition_number(expression: &str, start: usize) -> usize {
    expression[start..]
        .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .map_or(expression.len(), |offset| start + offset)
}

fn scan_condition_identifier(expression: &str, start: usize) -> usize {
    expression[start..]
        .find(|character: char| !c_identifier_char(character))
        .map_or(expression.len(), |offset| start + offset)
}

fn parse_integer_literal(literal: &str) -> Option<i128> {
    let literal = literal.trim_end_matches(['u', 'U', 'l', 'L']);
    if literal.is_empty() {
        return None;
    }
    let (radix, digits) = if let Some(digits) = literal
        .strip_prefix("0x")
        .or_else(|| literal.strip_prefix("0X"))
    {
        (16, digits)
    } else if let Some(digits) = literal
        .strip_prefix("0b")
        .or_else(|| literal.strip_prefix("0B"))
    {
        (2, digits)
    } else if literal.len() > 1 && literal.starts_with('0') {
        (8, &literal[1..])
    } else {
        (10, literal)
    };
    if digits.is_empty() || !digits.chars().all(|character| character.is_digit(radix)) {
        return None;
    }
    i128::from_str_radix(digits, radix).ok()
}

fn strip_c_comments(expression: &str) -> Option<String> {
    let mut stripped = String::with_capacity(expression.len());
    let mut index = 0usize;
    while index < expression.len() {
        let rest = &expression[index..];
        if rest.starts_with("/*") {
            let end = rest.find("*/")?;
            index += end + 2;
            continue;
        }
        if rest.starts_with("//") {
            break;
        }
        let character = rest.chars().next()?;
        stripped.push(character);
        index += character.len_utf8();
    }
    Some(stripped)
}

fn c_identifier_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

#[cfg(test)]
#[path = "conditions_tests.rs"]
mod tests;
