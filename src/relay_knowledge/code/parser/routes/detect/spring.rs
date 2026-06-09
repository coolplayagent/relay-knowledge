use std::collections::BTreeSet;

use super::RouteCandidate;

const MAX_SPRING_MAPPING_ANNOTATION_LINES: usize = 12;

pub(in crate::code::parser) fn detect_spring_routes(content: &str) -> Vec<RouteCandidate> {
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending_annotations: Vec<SpringPendingAnnotation> = Vec::new();
    let mut class_prefixes = Vec::<SpringClassPrefix>::new();
    let mut nested_type_scopes = Vec::<SpringNestedTypeScope>::new();
    let mut brace_depth = 0usize;
    let lines = java_code_lines_without_comments(content);
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        restore_closed_spring_nested_type_scopes(
            &mut class_prefixes,
            &mut nested_type_scopes,
            brace_depth,
        );
        if let Some(annotation_offset) = spring_route_annotation_offset(trimmed) {
            let (annotation_statement, annotation_lines) =
                spring_annotation_statement_from_offset(&lines, index, annotation_offset);
            let (spring_routes, annotation_tail) =
                spring_route_annotations_and_tail(&annotation_statement);
            if !spring_routes.is_empty() {
                if pending_request_mapping_can_be_prefix(&pending_annotations) {
                    class_prefixes = pending_request_mapping_prefixes(&pending_annotations);
                    pending_annotations.clear();
                }
                pending_annotations.extend(spring_routes);
                let method_tail = spring_tail_after_leading_annotations(annotation_tail);
                if let Some(method_name) = parse_java_method_def(method_tail) {
                    record_spring_pending_routes(
                        &mut routes,
                        &mut seen,
                        &class_prefixes,
                        &mut pending_annotations,
                        method_name,
                        index + 1,
                    );
                    update_java_brace_depth(method_tail, &mut brace_depth);
                }
                index += annotation_lines;
                continue;
            }
        }
        if !pending_annotations.is_empty() && trimmed.starts_with('@') {
            let (annotation_statement, annotation_lines) =
                spring_annotation_statement_from_offset(&lines, index, 0);
            let annotation_tail = spring_statement_after_annotation(&annotation_statement);
            let method_tail = spring_tail_after_leading_annotations(annotation_tail);
            if let Some(method_name) = parse_java_method_def(method_tail) {
                record_spring_pending_routes(
                    &mut routes,
                    &mut seen,
                    &class_prefixes,
                    &mut pending_annotations,
                    method_name,
                    index + 1,
                );
                update_java_brace_depth(method_tail, &mut brace_depth);
            } else {
                update_java_brace_depth(&annotation_statement, &mut brace_depth);
            }
            index += annotation_lines;
            continue;
        }
        if line_declares_java_type(trimmed) {
            if pending_request_mapping_can_be_prefix(&pending_annotations) {
                class_prefixes = pending_request_mapping_prefixes(&pending_annotations);
                nested_type_scopes.clear();
            } else if !class_prefixes.is_empty()
                && line_declares_nested_java_helper_type(trimmed, brace_depth)
            {
                nested_type_scopes.push(SpringNestedTypeScope {
                    restore_at_depth: brace_depth,
                    class_prefixes: class_prefixes.clone(),
                });
                class_prefixes.clear();
            } else {
                class_prefixes.clear();
                nested_type_scopes.clear();
            }
            pending_annotations.clear();
            update_java_brace_depth(trimmed, &mut brace_depth);
            restore_closed_spring_nested_type_scopes(
                &mut class_prefixes,
                &mut nested_type_scopes,
                brace_depth,
            );
            index += 1;
            continue;
        }
        if !pending_annotations.is_empty() {
            if let Some(method_name) = parse_java_method_def(trimmed) {
                record_spring_pending_routes(
                    &mut routes,
                    &mut seen,
                    &class_prefixes,
                    &mut pending_annotations,
                    method_name,
                    index + 1,
                );
            } else if !trimmed.is_empty()
                && !trimmed.starts_with("public")
                && !trimmed.starts_with("private")
                && !trimmed.starts_with("protected")
                && !trimmed.starts_with("@")
            {
                pending_annotations.clear();
            }
        }
        update_java_brace_depth(trimmed, &mut brace_depth);
        index += 1;
    }
    routes
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SpringAnnotationKind {
    RequestMapping,
    MethodMapping,
}

struct SpringPendingAnnotation {
    http_method: String,
    url: String,
    kind: SpringAnnotationKind,
}

#[derive(Clone)]
struct SpringClassPrefix {
    url: String,
    http_method: String,
}

struct SpringNestedTypeScope {
    restore_at_depth: usize,
    class_prefixes: Vec<SpringClassPrefix>,
}

fn record_spring_pending_routes(
    routes: &mut Vec<RouteCandidate>,
    seen: &mut BTreeSet<(String, String, String, usize)>,
    class_prefixes: &[SpringClassPrefix],
    pending_annotations: &mut Vec<SpringPendingAnnotation>,
    method_name: String,
    line: usize,
) {
    let prefixes = route_class_prefixes(class_prefixes);
    for annotation in pending_annotations.drain(..) {
        for prefix in &prefixes {
            let full_url = merge_url_parts(&prefix.url, &annotation.url);
            for http_method in route_http_methods_with_class_prefix(prefix, &annotation.http_method)
            {
                let key = (
                    full_url.clone(),
                    http_method.clone(),
                    method_name.clone(),
                    line,
                );
                if seen.insert(key) {
                    routes.push(RouteCandidate {
                        url: full_url.clone(),
                        http_method,
                        handler_name: method_name.clone(),
                        framework: "spring".to_owned(),
                        line,
                    });
                }
            }
        }
    }
}

fn pending_request_mapping_can_be_prefix(annotations: &[SpringPendingAnnotation]) -> bool {
    !annotations.is_empty()
        && annotations
            .iter()
            .all(|annotation| annotation.kind == SpringAnnotationKind::RequestMapping)
}

fn pending_request_mapping_prefixes(
    annotations: &[SpringPendingAnnotation],
) -> Vec<SpringClassPrefix> {
    let mut seen = BTreeSet::new();
    let mut prefixes = Vec::new();
    for annotation in annotations {
        let key = (annotation.url.clone(), annotation.http_method.clone());
        if seen.insert(key) {
            prefixes.push(SpringClassPrefix {
                url: annotation.url.clone(),
                http_method: annotation.http_method.clone(),
            });
        }
    }
    prefixes
}

fn route_class_prefixes(class_prefixes: &[SpringClassPrefix]) -> Vec<SpringClassPrefix> {
    if class_prefixes.is_empty() {
        return vec![SpringClassPrefix {
            url: String::new(),
            http_method: "any".to_owned(),
        }];
    }
    class_prefixes.to_vec()
}

fn route_http_methods_with_class_prefix(prefix: &SpringClassPrefix, method: &str) -> Vec<String> {
    if method == "any" {
        return vec![prefix.http_method.clone()];
    }
    if prefix.http_method == "any" || prefix.http_method == method {
        return vec![method.to_owned()];
    }
    vec![prefix.http_method.clone(), method.to_owned()]
}

fn spring_route_annotation_offset(line: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
            continue;
        }
        if character == '@'
            && spring_annotation_name_at(line, index).is_some_and(is_spring_route_annotation_name)
        {
            return Some(index);
        }
    }
    None
}

fn spring_route_annotations_and_tail(statement: &str) -> (Vec<SpringPendingAnnotation>, &str) {
    let mut annotations = Vec::new();
    let mut scan = statement.trim_start();
    while let Some(annotation_offset) = spring_route_annotation_offset(scan) {
        if !scan[..annotation_offset].trim().is_empty() {
            break;
        }
        let annotation_statement = &scan[annotation_offset..];
        let Some(mut spring_routes) = parse_spring_route_annotation(annotation_statement) else {
            break;
        };
        annotations.append(&mut spring_routes);
        scan = spring_statement_after_annotation(annotation_statement).trim_start();
    }
    (annotations, scan)
}

fn spring_annotation_name_at(line: &str, at_index: usize) -> Option<&str> {
    let after_at = &line[at_index + 1..];
    let name_end = after_at
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_at.len());
    let annotation_name = &after_at[..name_end];
    (!annotation_name.is_empty()).then(|| {
        annotation_name
            .rsplit('.')
            .next()
            .unwrap_or(annotation_name)
    })
}

fn is_spring_route_annotation_name(annotation: &str) -> bool {
    matches!(
        annotation,
        "GetMapping"
            | "PostMapping"
            | "PutMapping"
            | "DeleteMapping"
            | "PatchMapping"
            | "RequestMapping"
    )
}

fn spring_annotation_statement_from_offset(
    lines: &[String],
    start: usize,
    first_line_offset: usize,
) -> (String, usize) {
    let mut statement = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut saw_open = false;
    let mut consumed = 0usize;
    for line in lines
        .iter()
        .skip(start)
        .take(MAX_SPRING_MAPPING_ANNOTATION_LINES)
    {
        let trimmed = line.trim();
        let segment = if consumed == 0 {
            trimmed.get(first_line_offset..).unwrap_or(trimmed)
        } else {
            trimmed
        };
        if !statement.is_empty() {
            statement.push(' ');
        }
        statement.push_str(segment);
        consumed += 1;
        for character in segment.chars() {
            if let Some(quote_char) = quote {
                if escaped {
                    escaped = false;
                    continue;
                }
                if character == '\\' {
                    escaped = true;
                    continue;
                }
                if character == quote_char {
                    quote = None;
                }
                continue;
            }
            match character {
                '"' => quote = Some(character),
                '(' => {
                    depth += 1;
                    saw_open = true;
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    if saw_open && depth == 0 {
                        return (statement, consumed);
                    }
                }
                _ => {}
            }
        }
        if !saw_open {
            return (statement, consumed);
        }
    }
    (statement, consumed.max(1))
}

fn spring_statement_after_annotation(statement: &str) -> &str {
    let trimmed = statement.trim();
    if !trimmed.starts_with('@') {
        return "";
    }
    let after_at = &trimmed[1..];
    let name_end = after_at
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_at.len());
    let rest = after_at[name_end..].trim_start();
    if !rest.starts_with('(') {
        return rest;
    }
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in rest.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '"' => quote = Some(character),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return &rest[index + character.len_utf8()..];
                }
            }
            _ => {}
        }
    }
    ""
}

fn spring_tail_after_leading_annotations(mut tail: &str) -> &str {
    loop {
        let trimmed = tail.trim_start();
        if !trimmed.starts_with('@') {
            return trimmed;
        }
        let next_tail = spring_statement_after_annotation(trimmed);
        if next_tail.len() == trimmed.len() {
            return trimmed;
        }
        tail = next_tail;
    }
}

fn parse_spring_route_annotation(line: &str) -> Option<Vec<SpringPendingAnnotation>> {
    let annotation = extract_spring_annotation_name(line)?;
    match annotation {
        "GetMapping" => Some(spring_pending_annotations(
            vec!["get".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "PostMapping" => Some(spring_pending_annotations(
            vec!["post".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "PutMapping" => Some(spring_pending_annotations(
            vec!["put".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "DeleteMapping" => Some(spring_pending_annotations(
            vec!["delete".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "PatchMapping" => Some(spring_pending_annotations(
            vec!["patch".to_owned()],
            extract_annotation_string_values(line),
            SpringAnnotationKind::MethodMapping,
        )),
        "RequestMapping" => {
            let urls = extract_annotation_string_values(line);
            Some(spring_pending_annotations(
                extract_spring_method_attributes(line),
                urls,
                SpringAnnotationKind::RequestMapping,
            ))
        }
        _ => None,
    }
}

fn spring_pending_annotations(
    methods: Vec<String>,
    urls: Vec<String>,
    kind: SpringAnnotationKind,
) -> Vec<SpringPendingAnnotation> {
    let urls = if urls.is_empty() {
        vec![String::new()]
    } else {
        urls
    };
    let mut annotations = Vec::new();
    for method in methods {
        for url in &urls {
            annotations.push(SpringPendingAnnotation {
                http_method: method.clone(),
                url: url.clone(),
                kind,
            });
        }
    }
    annotations
}

fn extract_spring_annotation_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with('@') {
        return None;
    }
    let after_at = &trimmed[1..];
    let name_end = after_at
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_at.len());
    let annotation_name = &after_at[..name_end];
    Some(
        annotation_name
            .rsplit('.')
            .next()
            .unwrap_or(annotation_name),
    )
}

fn extract_annotation_string_values(line: &str) -> Vec<String> {
    let Some(paren_pos) = line.find('(') else {
        return Vec::new();
    };
    let inner_start = &line[paren_pos + 1..];
    let inner = inner_start.trim_start();
    if inner.starts_with('"') {
        extract_double_quoted_java_string(inner)
            .into_iter()
            .collect()
    } else if inner.starts_with('{') {
        extract_java_string_values_from_attribute_value(inner)
    } else {
        find_named_java_attribute_value(inner, "value")
            .or_else(|| find_named_java_attribute_value(inner, "path"))
            .map(extract_java_string_values_from_attribute_value)
            .unwrap_or_default()
    }
}

fn find_named_java_attribute_value<'a>(inner: &'a str, name: &str) -> Option<&'a str> {
    for argument in split_java_top_level_arguments(inner) {
        let Some(after_name) = argument.trim_start().strip_prefix(name) else {
            continue;
        };
        let after_name = after_name.trim_start();
        if let Some(after_eq) = after_name.strip_prefix('=') {
            return Some(after_eq.trim_start());
        }
    }
    None
}

fn split_java_top_level_arguments(inner: &str) -> Vec<&str> {
    let mut arguments = Vec::new();
    let mut argument_start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in inner.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                let argument = inner[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                return arguments;
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let argument = inner[argument_start..index].trim();
                if !argument.is_empty() {
                    arguments.push(argument);
                }
                argument_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    let argument = inner[argument_start..].trim();
    if !argument.is_empty() {
        arguments.push(argument);
    }
    arguments
}

fn extract_java_string_values_from_attribute_value(value: &str) -> Vec<String> {
    let segment = java_attribute_value_segment(value);
    if segment.trim_start().starts_with('{') {
        return extract_double_quoted_java_strings(segment);
    }
    extract_double_quoted_java_string(segment)
        .into_iter()
        .collect()
}

fn java_attribute_value_segment(value: &str) -> &str {
    let value = value.trim_start();
    if value.starts_with('{') {
        let mut depth = 0usize;
        for (index, character) in value.char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return &value[..=index];
                    }
                }
                _ => {}
            }
        }
        return value;
    }
    let end = value.find([',', ')']).unwrap_or(value.len());
    &value[..end]
}

fn extract_double_quoted_java_strings(s: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(relative_start) = s[offset..].find('"') {
        let start = offset + relative_start;
        let mut value = String::new();
        let mut escaped = false;
        let mut closed_at = None;
        for (relative_index, character) in s[start + 1..].char_indices() {
            if escaped {
                value.push(character);
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == '"' {
                closed_at = Some(start + 1 + relative_index + character.len_utf8());
                break;
            }
            value.push(character);
        }
        let Some(next_offset) = closed_at else {
            break;
        };
        values.push(value);
        offset = next_offset;
    }
    values
}

fn extract_double_quoted_java_string(s: &str) -> Option<String> {
    if !s.starts_with('"') {
        return None;
    }
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 1usize;
    while i < chars.len() {
        match chars[i] {
            '"' => break,
            '\\' => {
                if i + 1 < chars.len() {
                    result.push(chars[i + 1]);
                    i += 2;
                } else {
                    break;
                }
            }
            c => {
                result.push(c);
                i += 1;
            }
        }
    }
    Some(result)
}

fn extract_spring_method_attributes(line: &str) -> Vec<String> {
    let paren_pos = match line.find('(') {
        Some(pos) => pos,
        None => return vec!["any".to_owned()],
    };
    let inner = &line[paren_pos + 1..];
    let Some(after_eq) = find_named_java_attribute_value(inner, "method") else {
        return vec!["any".to_owned()];
    };
    let raw_value = java_attribute_value_segment(after_eq);
    let mut methods = Vec::new();
    for part in raw_value
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
    {
        let part = part.trim();
        let method_part = part.strip_prefix("RequestMethod.").unwrap_or(part);
        let method = method_part
            .trim_matches(|character: char| !character.is_ascii_alphabetic())
            .to_ascii_lowercase();
        if matches!(
            method.as_str(),
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
        ) {
            methods.push(method);
        }
    }
    if methods.is_empty() {
        methods.push("any".to_owned());
    }

    methods
}

fn line_declares_java_type(line: &str) -> bool {
    let declaration = line.split('{').next().unwrap_or(line);
    declaration.contains(" class ")
        || declaration.contains(" interface ")
        || declaration.contains(" enum ")
        || declaration.starts_with("class ")
        || declaration.starts_with("interface ")
        || declaration.starts_with("enum ")
}

fn line_declares_nested_java_helper_type(line: &str, brace_depth: usize) -> bool {
    if brace_depth == 0 {
        return false;
    }
    let declaration = line.split('{').next().unwrap_or(line).trim();
    line_declares_java_type(declaration)
}

fn restore_closed_spring_nested_type_scopes(
    class_prefixes: &mut Vec<SpringClassPrefix>,
    nested_type_scopes: &mut Vec<SpringNestedTypeScope>,
    brace_depth: usize,
) {
    while nested_type_scopes
        .last()
        .is_some_and(|scope| brace_depth <= scope.restore_at_depth)
    {
        if let Some(scope) = nested_type_scopes.pop() {
            *class_prefixes = scope.class_prefixes;
        }
    }
}

fn update_java_brace_depth(line: &str, brace_depth: &mut usize) {
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '{' => *brace_depth += 1,
            '}' => *brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }
}

fn java_code_lines_without_comments(content: &str) -> Vec<String> {
    let mut state = JavaCodeLineState::default();
    content
        .lines()
        .map(|line| java_code_line_without_comments(line, &mut state))
        .collect()
}

#[derive(Default)]
struct JavaCodeLineState {
    in_block_comment: bool,
    in_text_block: bool,
}

fn java_code_line_without_comments(line: &str, state: &mut JavaCodeLineState) -> String {
    let mut result = String::new();
    let mut chars = line.char_indices().peekable();
    let mut quote = None;
    let mut escaped = false;
    while let Some((index, character)) = chars.next() {
        if state.in_block_comment {
            if character == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                chars.next();
                state.in_block_comment = false;
            }
            continue;
        }
        if state.in_text_block {
            if line[index..].starts_with("\"\"\"") {
                chars.next();
                chars.next();
                state.in_text_block = false;
            }
            continue;
        }
        if let Some(quote_char) = quote {
            result.push(character);
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote_char {
                quote = None;
            }
            continue;
        }
        if character == '/' && chars.peek().is_some_and(|(_, next)| *next == '*') {
            chars.next();
            state.in_block_comment = true;
            continue;
        }
        if character == '/' && chars.peek().is_some_and(|(_, next)| *next == '/') {
            break;
        }
        if character == '"' && line[index..].starts_with("\"\"\"") {
            chars.next();
            chars.next();
            state.in_text_block = true;
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
        }
        result.push(character);
    }
    result
}

fn parse_java_method_def(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let marker = trimmed.find('(')?;
    let before_paren = &trimmed[..marker];
    let name_start = before_paren
        .rfind(|c: char| c.is_whitespace() || c == '<' || c == '>')
        .map_or(0, |pos| pos + 1);
    let name = &before_paren[name_start..];
    if name.is_empty() || name.chars().next().is_some_and(|c| !c.is_alphanumeric()) {
        return None;
    }
    Some(name.to_owned())
}

fn merge_url_parts(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        if suffix.is_empty() {
            return "/".to_owned();
        }
        return if suffix.starts_with('/') {
            suffix.to_owned()
        } else {
            format!("/{suffix}")
        };
    }
    if suffix.is_empty() {
        return prefix.to_owned();
    }
    let p = prefix.trim_end_matches('/');
    let s = suffix.trim_start_matches('/');
    format!("{p}/{s}")
}
