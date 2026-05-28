use std::collections::HashSet;

use super::{DependencySeed, SeedInput, push_seed, strip_comment};

pub(super) fn parse_go_mod(content: &str, records: &mut Vec<DependencySeed>) {
    let mut in_require_block = false;
    for (index, line) in content.lines().enumerate() {
        let trimmed = strip_comment(line, '/').trim();
        if trimmed == "require (" {
            in_require_block = true;
            continue;
        }
        if in_require_block && trimmed == ")" {
            in_require_block = false;
            continue;
        }
        let require_line = if let Some(rest) = trimmed.strip_prefix("require ") {
            rest
        } else if in_require_block {
            trimmed
        } else {
            continue;
        };
        let mut parts = require_line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        let version = parts.next().map(str::to_owned);
        if !name.contains('.') {
            continue;
        }
        push_seed(
            records,
            SeedInput::new(
                "go",
                "go",
                name.to_owned(),
                version,
                "require",
                "go.mod",
                false,
            )
            .line(index + 1)
            .excerpt(require_line),
        );
    }
}

pub(super) fn parse_go_sum(content: &str, records: &mut Vec<DependencySeed>) {
    let mut seen = HashSet::new();
    for (index, line) in content.lines().enumerate() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(version)) = (parts.next(), parts.next()) else {
            continue;
        };
        let version = version.strip_suffix("/go.mod").unwrap_or(version);
        if !name.contains('.') {
            continue;
        }
        if !seen.insert((name.to_owned(), version.to_owned())) {
            continue;
        }
        push_seed(
            records,
            SeedInput::new("go", "go", name.to_owned(), None, "locked", "go.sum", true)
                .resolved(Some(version.to_owned()))
                .line(index + 1)
                .excerpt(line.trim()),
        );
    }
}
