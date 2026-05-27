use super::FeatureFlagFileInput;

#[derive(Default)]
pub(super) struct CommentState {
    block_depth: usize,
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

            if let Some(lifetime_len) = rust_lifetime_token_len(input.language_id, rest) {
                scan_line.push_str(&rest[1..lifetime_len]);
                index = index.saturating_add(lifetime_len);
                continue;
            }
            if character == '"' || character == '\'' {
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

        (!scan_line.trim().is_empty()).then_some(scan_line)
    }
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
