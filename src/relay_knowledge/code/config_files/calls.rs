use super::{
    model::ConfigRange,
    source::{
        ConfigLine, call_args_prefix, source_lines, strip_inline_hash_comment, valid_config_key,
    },
};

pub(super) struct TextCall {
    pub(super) text: String,
    pub(super) range: ConfigRange,
}

pub(super) fn starlark_calls(content: &str, command: &str) -> Vec<TextCall> {
    let mut calls = Vec::new();
    let mut pending: Option<TextCallBuilder> = None;
    for line in source_lines(content) {
        if let Some(builder) = &mut pending {
            builder.push_continuation(&line);
            if builder.closed()
                && let Some(call) = pending.take().map(TextCallBuilder::build)
            {
                calls.push(call);
            }
            continue;
        }

        let trimmed = line.text.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let code = strip_inline_hash_comment(trimmed).trim_end();
        let Some(rest) = call_args_prefix(code, command) else {
            continue;
        };
        let mut builder = TextCallBuilder::new(line.range());
        builder.push_text(rest);
        if builder.closed() {
            calls.push(builder.build());
        } else {
            pending = Some(builder);
        }
    }

    calls
}

struct TextCallBuilder {
    text: String,
    range: ConfigRange,
    balance: i32,
}

impl TextCallBuilder {
    fn new(range: ConfigRange) -> Self {
        Self {
            text: String::new(),
            range,
            balance: 1,
        }
    }

    fn push_continuation(&mut self, line: &ConfigLine<'_>) {
        self.range.byte_end = line.byte_end;
        self.range.line_end = line.number;
        self.push_text(strip_inline_hash_comment(line.text));
    }

    fn push_text(&mut self, text: &str) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(text.trim());
        self.balance += text_paren_delta(text);
    }

    fn closed(&self) -> bool {
        self.balance <= 0
    }

    fn build(self) -> TextCall {
        TextCall {
            text: self.text,
            range: self.range,
        }
    }
}

fn text_paren_delta(value: &str) -> i32 {
    let mut delta = 0;
    let mut quote = None;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if quote.is_none() {
            match character {
                '(' => delta += 1,
                ')' => delta -= 1,
                _ => {}
            }
        }
    }

    delta
}

pub(super) struct CmakeCall {
    pub(super) command: String,
    pub(super) args: String,
    pub(super) range: ConfigRange,
}

pub(super) fn cmake_calls(content: &str) -> Vec<CmakeCall> {
    let mut calls = Vec::new();
    let mut pending: Option<CmakeCallBuilder> = None;
    let mut bracket_comment_end = None;
    for line in source_lines(content) {
        let code = cmake_code_before_bracket_comment(line.text, &mut bracket_comment_end);
        if code.trim().is_empty() {
            continue;
        }
        if let Some(builder) = &mut pending {
            builder.push_continuation(&line, code);
            if builder.closed()
                && let Some(call) = pending.take().and_then(CmakeCallBuilder::build)
            {
                calls.push(call);
            }
            continue;
        }

        let trimmed = code.trim_start();
        let Some((command, rest)) = cmake_call_start(trimmed) else {
            continue;
        };
        let mut builder = CmakeCallBuilder::new(command, line.range());
        builder.push_args(rest);
        if builder.closed() {
            if let Some(call) = builder.build() {
                calls.push(call);
            }
        } else {
            pending = Some(builder);
        }
    }

    calls
}

struct CmakeCallBuilder {
    command: String,
    args: String,
    range: ConfigRange,
    balance: i32,
}

impl CmakeCallBuilder {
    fn new(command: &str, range: ConfigRange) -> Self {
        Self {
            command: command.to_ascii_lowercase(),
            args: String::new(),
            range,
            balance: 1,
        }
    }

    fn push_continuation(&mut self, line: &ConfigLine<'_>, text: &str) {
        self.range.byte_end = line.byte_end;
        self.range.line_end = line.number;
        self.push_args(text);
    }

    fn push_args(&mut self, text: &str) {
        let text = strip_inline_hash_comment(text);
        let mut chunk = text;
        if let Some(index) = text.rfind(')') {
            chunk = &text[..index];
        }
        if !self.args.is_empty() {
            self.args.push(' ');
        }
        self.args.push_str(chunk.trim());
        self.balance += cmake_paren_delta(text);
    }

    fn closed(&self) -> bool {
        self.balance <= 0
    }

    fn build(self) -> Option<CmakeCall> {
        (!self.args.trim().is_empty()).then(|| CmakeCall {
            command: self.command,
            args: self.args.trim().to_owned(),
            range: self.range,
        })
    }
}

fn cmake_call_start(line: &str) -> Option<(&str, &str)> {
    let (command, rest) = line.split_once('(')?;
    let command = command.trim();
    valid_config_key(command).then_some((command, rest))
}

fn cmake_code_before_bracket_comment<'a>(
    line: &'a str,
    bracket_comment_end: &mut Option<String>,
) -> &'a str {
    if let Some(marker) = bracket_comment_end {
        let Some(end) = line.find(marker.as_str()) else {
            return "";
        };
        let marker_len = marker.len();
        *bracket_comment_end = None;
        return &line[end + marker_len..];
    }

    let Some(index) = hash_outside_quotes(line) else {
        return line;
    };
    let after_hash = &line[index + '#'.len_utf8()..];
    let Some(marker) = cmake_bracket_end(after_hash) else {
        return line;
    };
    if !after_hash[marker.len()..].contains(marker.as_str()) {
        *bracket_comment_end = Some(marker);
    }

    &line[..index]
}

fn hash_outside_quotes(value: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if character == '#' && quote.is_none() {
            return Some(index);
        }
    }

    None
}

fn cmake_paren_delta(value: &str) -> i32 {
    let mut delta = 0;
    let mut index = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut bracket_end: Option<String> = None;
    while index < value.len() {
        let rest = &value[index..];
        if let Some(end) = &bracket_end {
            if rest.starts_with(end) {
                index += end.len();
                bracket_end = None;
            } else {
                index += rest.chars().next().map_or(1, char::len_utf8);
            }
            continue;
        }
        let character = rest
            .chars()
            .next()
            .expect("index should point to a character boundary");
        if escaped {
            escaped = false;
            index += character.len_utf8();
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            index += character.len_utf8();
            continue;
        }
        if matches!(character, '"' | '\'') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            index += character.len_utf8();
            continue;
        }
        if quote.is_none() {
            if let Some(end) = cmake_bracket_end(rest) {
                index += end.len();
                bracket_end = Some(end);
                continue;
            }
            match character {
                '(' => delta += 1,
                ')' => delta -= 1,
                _ => {}
            }
        }
        index += character.len_utf8();
    }

    delta
}

fn cmake_bracket_end(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'[') {
        return None;
    }
    let mut index = 1;
    while bytes.get(index) == Some(&b'=') {
        index += 1;
    }
    if bytes.get(index) == Some(&b'[') {
        Some(format!("]{}]", "=".repeat(index - 1)))
    } else {
        None
    }
}
