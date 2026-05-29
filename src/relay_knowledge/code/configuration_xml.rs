#[derive(Default)]
pub(super) struct ScanState {
    in_comment: bool,
    in_cdata: bool,
    pending_start_tag: String,
}

pub(super) fn element_names<'a>(line: &'a str, state: &mut ScanState) -> Vec<&'a str> {
    let mut names = Vec::new();
    let mut rest = line;
    loop {
        if state.in_comment {
            let Some(end) = rest.find("-->") else {
                return names;
            };
            state.in_comment = false;
            rest = &rest[end + "-->".len()..];
        }
        if state.in_cdata {
            let Some(end) = rest.find("]]>") else {
                return names;
            };
            state.in_cdata = false;
            rest = &rest[end + "]]>".len()..];
        }

        let Some(index) = rest.find('<') else {
            return names;
        };
        let after = &rest[index + 1..];
        if let Some(next) = after.strip_prefix('/') {
            rest = next;
            continue;
        }
        let skip_marker = if after.starts_with("![CDATA[") {
            Some("]]>")
        } else if after.starts_with("!--") {
            Some("-->")
        } else if after.starts_with('?') {
            Some("?>")
        } else if after.starts_with('!') {
            Some(">")
        } else {
            None
        };
        if after.starts_with("![CDATA[") {
            let Some(end) = after.find("]]>") else {
                state.in_cdata = true;
                return names;
            };
            rest = &after[end + "]]>".len()..];
            continue;
        }
        if after.starts_with("!--") {
            let Some(end) = after.find("-->") else {
                state.in_comment = true;
                return names;
            };
            rest = &after[end + "-->".len()..];
            continue;
        }
        if let Some(marker) = skip_marker {
            rest = after
                .find(marker)
                .map(|end| &after[end + marker.len()..])
                .unwrap_or("");
            continue;
        }
        if let Some(name) = name(after) {
            names.push(name);
        }
        rest = after.find('>').map(|end| &after[end + 1..]).unwrap_or("");
    }
}

pub(super) fn strip_ignored(value: &str, state: &mut ScanState) -> String {
    let mut output = String::new();
    let mut rest = value;
    loop {
        if state.in_comment {
            let Some(end) = rest.find("-->") else {
                return output;
            };
            state.in_comment = false;
            rest = &rest[end + "-->".len()..];
        }
        if state.in_cdata {
            let Some(end) = rest.find("]]>") else {
                return output;
            };
            state.in_cdata = false;
            rest = &rest[end + "]]>".len()..];
        }
        let Some((start, marker)) = next_ignored_marker(rest) else {
            output.push_str(rest);
            return output;
        };
        output.push_str(&rest[..start]);
        match marker {
            IgnoredMarker::Comment => {
                rest = &rest[start + "<!--".len()..];
                state.in_comment = true;
            }
            IgnoredMarker::Cdata => {
                rest = &rest[start + "<![CDATA[".len()..];
                state.in_cdata = true;
            }
        }
    }
}

pub(super) fn import_modules(line: &str, state: &mut ScanState) -> Vec<String> {
    let text = strip_ignored(line, state);
    let mut modules = Vec::new();

    for tag in start_tags(&text, state) {
        for value in attribute_values(&tag, "schemaLocation") {
            modules.extend(schema_location_paths(value).map(str::to_owned));
        }
        for value in attribute_values(&tag, "noNamespaceSchemaLocation") {
            modules.push(value.to_owned());
        }
        for attribute in ["href", "location"] {
            for value in attribute_values(&tag, attribute) {
                modules.push(value.to_owned());
            }
        }
    }

    modules
}

#[derive(Clone, Copy)]
enum IgnoredMarker {
    Comment,
    Cdata,
}

fn next_ignored_marker(value: &str) -> Option<(usize, IgnoredMarker)> {
    match (value.find("<!--"), value.find("<![CDATA[")) {
        (Some(comment), Some(cdata)) if comment <= cdata => Some((comment, IgnoredMarker::Comment)),
        (Some(_), Some(cdata)) => Some((cdata, IgnoredMarker::Cdata)),
        (Some(comment), None) => Some((comment, IgnoredMarker::Comment)),
        (None, Some(cdata)) => Some((cdata, IgnoredMarker::Cdata)),
        (None, None) => None,
    }
}

fn schema_location_paths(value: &str) -> impl Iterator<Item = &str> {
    value
        .split_whitespace()
        .enumerate()
        .filter_map(|(index, part)| (index % 2 == 1).then_some(part))
}

fn start_tags(line: &str, state: &mut ScanState) -> Vec<String> {
    let mut tags = Vec::new();
    let mut rest = line;
    if !state.pending_start_tag.is_empty() {
        let Some(end) = rest.find('>') else {
            append_pending_start_tag(&mut state.pending_start_tag, rest);
            return tags;
        };
        append_pending_start_tag(&mut state.pending_start_tag, &rest[..end]);
        tags.push(std::mem::take(&mut state.pending_start_tag));
        rest = &rest[end + '>'.len_utf8()..];
    }

    while let Some(index) = rest.find('<') {
        let after = &rest[index + '<'.len_utf8()..];
        if after
            .chars()
            .next()
            .is_some_and(|character| matches!(character, '/' | '?' | '!'))
        {
            rest = after.find('>').map(|end| &after[end + 1..]).unwrap_or("");
            continue;
        }
        let Some(end) = after.find('>') else {
            append_pending_start_tag(&mut state.pending_start_tag, after);
            break;
        };
        tags.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }

    tags
}

fn append_pending_start_tag(tag: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !tag.is_empty() {
        tag.push(' ');
    }
    tag.push_str(text);
}

fn attribute_values<'a>(line: &'a str, name: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    let mut search_start = 0;
    while let Some(relative_index) = line[search_start..].find(name) {
        let index = search_start + relative_index;
        search_start = index + name.len();
        if !attribute_name_starts_at(line, index) {
            continue;
        }
        let after_name = line[index + name.len()..].trim_start();
        if !after_name.starts_with('=') {
            continue;
        }
        let after_equals = after_name['='.len_utf8()..].trim_start();
        if let Some((value, after)) = first_quoted_with_rest(after_equals) {
            values.push(value);
            search_start = line.len() - after.len();
        } else {
            break;
        }
    }

    values
}

fn attribute_name_starts_at(line: &str, index: usize) -> bool {
    line[..index]
        .chars()
        .next_back()
        .is_none_or(|character| character.is_whitespace() || matches!(character, '<' | '/' | ':'))
}

fn first_quoted_with_rest(value: &str) -> Option<(&str, &str)> {
    let quote = value
        .chars()
        .find(|character| matches!(character, '"' | '\''))?;
    let start = value.find(quote)? + quote.len_utf8();
    let rest = &value[start..];
    let end = rest.find(quote)?;

    Some((&rest[..end], &rest[end + quote.len_utf8()..]))
}

fn name(value: &str) -> Option<&str> {
    let end = value
        .find(|character: char| character.is_whitespace() || matches!(character, '>' | '/'))
        .unwrap_or(value.len());
    let name = &value[..end];
    (!name.is_empty()).then_some(name)
}
