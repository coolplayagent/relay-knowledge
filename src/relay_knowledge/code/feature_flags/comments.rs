use super::FeatureFlagFileInput;

#[derive(Default)]
pub(super) struct CommentState {
    block_depth: usize,
    template_literal_open: bool,
    template_interpolation_depth: usize,
}

impl CommentState {
    pub(super) fn scan_line(
        &mut self,
        line: &str,
        input: &FeatureFlagFileInput<'_>,
    ) -> Option<String> {
        let mut scan_line = String::new();
        let hash_comment = hash_starts_comment(input);
        let nested_blocks = nested_block_comments(input.language_id);
        let mut quote = None;
        let mut escaped = false;
        let mut index = 0usize;
        if self.template_literal_open {
            let continuation = self.consume_template_continuation(line);
            scan_line.push_str(&continuation.scan_line);
            index = continuation.next_index;
            if self.template_literal_open {
                return (!scan_line.trim().is_empty()).then_some(scan_line);
            }
        }

        while index < line.len() {
            let rest = &line[index..];
            if self.block_depth > 0 {
                if nested_blocks && rest.starts_with("/*") {
                    self.block_depth = self.block_depth.saturating_add(1);
                    index = index.saturating_add(2);
                    continue;
                }
                if rest.starts_with("*/") {
                    self.block_depth = self.block_depth.saturating_sub(1);
                    index = index.saturating_add(2);
                    continue;
                }
                let Some(character) = rest.chars().next() else {
                    break;
                };
                index = index.saturating_add(character.len_utf8());
                continue;
            }

            let Some(character) = rest.chars().next() else {
                break;
            };
            if let Some(quote_character) = quote {
                if quote_character == '`' && rest.starts_with("${") {
                    self.template_interpolation_depth =
                        self.template_interpolation_depth.saturating_add(1);
                    scan_line.push_str("${");
                    index = index.saturating_add(2);
                    continue;
                }
                scan_line.push(character);
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if quote_character == '`' && self.template_interpolation_depth > 0 {
                    if character == '{' {
                        self.template_interpolation_depth =
                            self.template_interpolation_depth.saturating_add(1);
                    } else if character == '}' {
                        self.template_interpolation_depth =
                            self.template_interpolation_depth.saturating_sub(1);
                    }
                } else if character == quote_character {
                    quote = None;
                }
                index = index.saturating_add(character.len_utf8());
                continue;
            }

            if let Some(lifetime_len) = rust_lifetime_token_len(input.language_id, rest) {
                scan_line.push_str(&rest[1..lifetime_len]);
                index = index.saturating_add(lifetime_len);
                continue;
            }
            if character == '"' || character == '\'' || character == '`' {
                quote = Some(character);
                scan_line.push(character);
                index = index.saturating_add(character.len_utf8());
                continue;
            }
            if rest.starts_with("/*") {
                self.block_depth = 1;
                index = index.saturating_add(2);
                continue;
            }
            if rest.starts_with("//") || (hash_comment && character == '#') {
                break;
            }

            scan_line.push(character);
            index = index.saturating_add(character.len_utf8());
        }
        if quote == Some('`') {
            self.template_literal_open = true;
        }

        (!scan_line.trim().is_empty()).then_some(scan_line)
    }

    fn consume_template_continuation(&mut self, line: &str) -> TemplateContinuation {
        let mut scan_line = String::new();
        let mut index = 0usize;
        let mut escaped = false;
        while index < line.len() {
            if self.template_interpolation_depth > 0 {
                let interpolation = self.consume_template_interpolation(line, index);
                scan_line.push_str(&interpolation.scan_line);
                index = interpolation.next_index;
                if self.template_interpolation_depth > 0 {
                    return TemplateContinuation {
                        scan_line,
                        next_index: index,
                    };
                }
                continue;
            }
            let rest = &line[index..];
            let Some(character) = rest.chars().next() else {
                break;
            };
            if escaped {
                scan_line.push(' ');
                escaped = false;
                index = index.saturating_add(character.len_utf8());
                continue;
            }
            if character == '\\' {
                scan_line.push(' ');
                escaped = true;
                index = index.saturating_add(character.len_utf8());
                continue;
            }
            if character == '`' {
                self.template_literal_open = false;
                scan_line.push(' ');
                index = index.saturating_add(character.len_utf8());
                return TemplateContinuation {
                    scan_line,
                    next_index: index,
                };
            }
            if rest.starts_with("${") {
                self.template_interpolation_depth =
                    self.template_interpolation_depth.saturating_add(1);
                scan_line.push_str("  ");
                index = index.saturating_add(2);
                let interpolation = self.consume_template_interpolation(line, index);
                scan_line.push_str(&interpolation.scan_line);
                index = interpolation.next_index;
                continue;
            }
            scan_line.push(' ');
            index = index.saturating_add(character.len_utf8());
        }

        TemplateContinuation {
            scan_line,
            next_index: index,
        }
    }

    fn consume_template_interpolation(&mut self, line: &str, start: usize) -> TemplateContinuation {
        let mut scan_line = String::new();
        let mut quote = None;
        let mut escaped = false;
        let mut index = start;
        while index < line.len() {
            let rest = &line[index..];
            let Some(character) = rest.chars().next() else {
                break;
            };
            if let Some(quote_character) = quote {
                scan_line.push(character);
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == quote_character {
                    quote = None;
                }
                index = index.saturating_add(character.len_utf8());
                continue;
            }
            if character == '"' || character == '\'' {
                quote = Some(character);
                scan_line.push(character);
                index = index.saturating_add(character.len_utf8());
                continue;
            }
            if rest.starts_with("${") {
                self.template_interpolation_depth =
                    self.template_interpolation_depth.saturating_add(1);
                scan_line.push_str("  ");
                index = index.saturating_add(2);
                continue;
            }
            if character == '{' {
                self.template_interpolation_depth =
                    self.template_interpolation_depth.saturating_add(1);
                scan_line.push(character);
                index = index.saturating_add(character.len_utf8());
                continue;
            }
            if character == '}' {
                self.template_interpolation_depth =
                    self.template_interpolation_depth.saturating_sub(1);
                if self.template_interpolation_depth == 0 {
                    index = index.saturating_add(character.len_utf8());
                    return TemplateContinuation {
                        scan_line,
                        next_index: index,
                    };
                }
                scan_line.push(character);
                index = index.saturating_add(character.len_utf8());
                continue;
            }
            scan_line.push(character);
            index = index.saturating_add(character.len_utf8());
        }

        TemplateContinuation {
            scan_line,
            next_index: index,
        }
    }
}

struct TemplateContinuation {
    scan_line: String,
    next_index: usize,
}

fn nested_block_comments(language_id: &str) -> bool {
    matches!(language_id, "kotlin" | "rust" | "scala" | "swift")
}

fn rust_lifetime_token_len(language_id: &str, value: &str) -> Option<usize> {
    if language_id != "rust" || !value.starts_with('\'') {
        return None;
    }

    let mut chars = value.char_indices();
    chars.next();
    let (first_index, first) = chars.next()?;
    if first != '_' && !first.is_ascii_alphabetic() {
        return None;
    }

    let mut end = first_index.saturating_add(first.len_utf8());
    for (index, character) in chars {
        if character.is_ascii_alphanumeric() || character == '_' {
            end = index.saturating_add(character.len_utf8());
        } else {
            break;
        }
    }

    if value[end..].starts_with('\'') {
        return None;
    }

    Some(end)
}

fn hash_starts_comment(input: &FeatureFlagFileInput<'_>) -> bool {
    if super::config::looks_like_config_file(input.path) {
        return true;
    }

    matches!(
        input.language_id,
        "python" | "ruby" | "bash" | "php" | "unknown"
    )
}
