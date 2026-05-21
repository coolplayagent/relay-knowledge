pub(super) fn call_excerpt(caller_excerpt: Option<&str>, caller: &str, callee: &str) -> String {
    let summary = format!("{caller} calls {callee}");
    let Some(site) = caller_excerpt
        .map(str::trim)
        .filter(|excerpt| !excerpt.is_empty())
        .map(|excerpt| call_site_excerpt(excerpt, callee, false))
    else {
        return summary;
    };

    if site.is_empty() || site == summary {
        summary
    } else {
        format!("{summary}: {site}")
    }
}

pub(super) fn reference_excerpt(source_excerpt: Option<&str>, kind: &str, name: &str) -> String {
    let summary = format!("{kind} reference to {name}");
    let Some(site) = source_excerpt
        .map(str::trim)
        .filter(|excerpt| !excerpt.is_empty())
        .map(|excerpt| reference_site_excerpt(excerpt, name))
    else {
        return summary;
    };

    if site.is_empty() || site == summary {
        summary
    } else {
        format!("{summary}: {site}")
    }
}

pub(super) fn callee_excerpt(caller_excerpt: Option<&str>, caller: &str, callee: &str) -> String {
    let summary = format!("{caller} calls {callee}");
    let Some(site) = caller_excerpt
        .map(str::trim)
        .filter(|excerpt| !excerpt.is_empty())
        .map(|excerpt| call_site_excerpt(excerpt, callee, true))
    else {
        return summary;
    };

    if site.is_empty() || site == summary {
        summary
    } else {
        format!("{summary}: {site}")
    }
}

fn call_site_excerpt(
    caller_excerpt: &str,
    callee: &str,
    include_execution_context: bool,
) -> String {
    if let Some((index, _)) = caller_excerpt
        .lines()
        .enumerate()
        .find(|(_, line)| line_declares_local_callable(line, callee))
    {
        return call_context_excerpt(caller_excerpt, index, include_execution_context, true);
    }

    caller_excerpt
        .lines()
        .enumerate()
        .find(|(_, line)| line_looks_like_call_to(line, callee))
        .or_else(|| {
            caller_excerpt
                .lines()
                .enumerate()
                .find(|(_, line)| line_contains_identifier(line, callee))
        })
        .map(|(index, line)| {
            if include_execution_context {
                call_context_excerpt(caller_excerpt, index, include_execution_context, false)
            } else {
                compact_excerpt_line(line)
            }
        })
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| compact_excerpt_line(caller_excerpt))
}

const MAX_CALLEE_CONTEXT_LINES: usize = 10;
const MAX_LOCAL_CALLABLE_CONTEXT_LINES: usize = 6;

fn call_context_excerpt(
    caller_excerpt: &str,
    start_index: usize,
    include_execution_context: bool,
    local_callable: bool,
) -> String {
    if include_execution_context {
        let limit = if local_callable {
            MAX_LOCAL_CALLABLE_CONTEXT_LINES
        } else {
            MAX_CALLEE_CONTEXT_LINES
        };
        compact_context_lines(caller_excerpt, start_index, limit, local_callable)
    } else {
        caller_excerpt
            .lines()
            .nth(start_index)
            .map(compact_excerpt_line)
            .unwrap_or_default()
    }
}

fn compact_context_lines(
    caller_excerpt: &str,
    start_index: usize,
    limit: usize,
    stop_at_balanced_body: bool,
) -> String {
    let mut selected = Vec::new();
    let mut brace_depth = 0i32;
    let mut saw_body = false;
    for line in caller_excerpt.lines().skip(start_index) {
        let compact = compact_excerpt_line(line);
        if !compact.is_empty() {
            selected.push(compact);
        }
        let (opens, closes) = brace_delta(line);
        brace_depth += opens as i32 - closes as i32;
        saw_body |= opens > 0;
        if selected.len() >= limit {
            break;
        }
        if stop_at_balanced_body && saw_body && brace_depth <= 0 {
            break;
        }
    }

    selected.join(" ")
}

fn brace_delta(line: &str) -> (usize, usize) {
    line.chars().fold((0, 0), |(opens, closes), character| {
        if character == '{' {
            (opens + 1, closes)
        } else if character == '}' {
            (opens, closes + 1)
        } else {
            (opens, closes)
        }
    })
}

fn reference_site_excerpt(source_excerpt: &str, name: &str) -> String {
    source_excerpt
        .lines()
        .find(|line| line_contains_identifier(line, name))
        .map(compact_excerpt_line)
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| compact_excerpt_line(source_excerpt))
}

fn line_looks_like_call_to(line: &str, callee: &str) -> bool {
    identifier_match_ranges(line, callee).any(|(_, end)| {
        let suffix = line[end..].trim_start();
        suffix.starts_with('(') || (suffix.starts_with('<') && suffix.contains('('))
    })
}

fn line_contains_identifier(line: &str, identifier: &str) -> bool {
    identifier_match_ranges(line, identifier).next().is_some()
}

pub(super) fn line_declares_local_callable(line: &str, callee_name: &str) -> bool {
    let Some((_, end)) = identifier_match_ranges(line, callee_name).next() else {
        return false;
    };
    callable_initializer_suffix(&line[end..])
}

fn callable_initializer_suffix(suffix: &str) -> bool {
    let suffix = suffix.trim_start();
    let initializer = suffix
        .strip_prefix(":=")
        .or_else(|| suffix.strip_prefix('='));
    let Some(initializer) = initializer else {
        return false;
    };
    let initializer = initializer.trim_start();

    initializer.contains("=>")
        || initializer.contains("lambda")
        || initializer.contains("func(")
        || initializer.contains("func ")
        || initializer.contains("](")
        || initializer.contains("] (")
        || initializer.contains("[]")
}

fn identifier_match_ranges<'a>(
    line: &'a str,
    identifier: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    (!identifier.is_empty())
        .then_some(())
        .into_iter()
        .flat_map(move |_| line.match_indices(identifier))
        .filter_map(move |(start, _)| {
            let end = start + identifier.len();
            (has_identifier_boundary_before(line, start)
                && has_identifier_boundary_after(line, end))
            .then_some((start, end))
        })
}

fn has_identifier_boundary_before(line: &str, start: usize) -> bool {
    line[..start]
        .chars()
        .next_back()
        .is_none_or(|character| !is_identifier_character(character))
}

fn has_identifier_boundary_after(line: &str, end: usize) -> bool {
    line[end..]
        .chars()
        .next()
        .is_none_or(|character| !is_identifier_character(character))
}

fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn compact_excerpt_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}
