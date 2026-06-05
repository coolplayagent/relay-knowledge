use std::collections::BTreeMap;

mod parameters;
mod sdk_methods;
mod templates;

pub(super) use parameters::{
    ParameterBodyStatus, function_parameter_body_status, function_parameter_receivers,
};
use sdk_methods::{SDK_FLAG_METHODS, sdk_flag_argument_index};
use templates::template_interpolation_code;

const CONFIG_RECEIVERS: &[&str] = &[
    "config",
    "settings",
    "feature_flags",
    "flags",
    "toggles",
    "options",
];
const CONFIG_METHODS: &[&str] = &[
    ".get(",
    ".get_bool(",
    ".getBoolean(",
    ".get_boolean(",
    ".enabled(",
    ".is_enabled(",
];
pub(super) fn env_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let line = template_interpolation_code(line);
    let line = line.as_str();
    for pattern in [
        "std::env::var(",
        "std::env::var_os(",
        "env::var(",
        "std::getenv(",
        "getenv(",
        "os.getenv(",
        "os.environ.get(",
        "System.getenv(",
        "Deno.env.get(",
    ] {
        collect_quoted_arguments(line, pattern, &mut keys);
    }
    collect_dotted_members(line, "process.env.", &mut keys);
    collect_dotted_members(line, "Bun.env.", &mut keys);
    collect_dotted_members(line, "import.meta.env.", &mut keys);
    collect_bracket_keys(line, "process.env[", &mut keys);
    collect_bracket_keys(line, "Bun.env[", &mut keys);
    collect_bracket_keys(line, "import.meta.env[", &mut keys);
    collect_bracket_keys(line, "os.environ[", &mut keys);
    collect_bounded_quoted_arguments(line, "ENV[", &mut keys);
    collect_bracket_keys(line, "$_ENV[", &mut keys);

    keys
}

pub(super) fn config_read_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for receiver in CONFIG_RECEIVERS {
        for method in CONFIG_METHODS {
            collect_quoted_arguments(line, &format!("{receiver}{method}"), &mut keys);
        }
    }

    keys
}

fn sdk_client_receiver_assignment(line: &str) -> Option<(String, bool)> {
    let assignment = assignment_operator_index(line)?;
    let rhs = &line[assignment.saturating_add(1)..];
    let is_sdk_client = sdk_provider_initializer(rhs);
    let receiver = assignment_receiver_name(&line[..assignment], true)?;

    Some((receiver, is_sdk_client))
}

fn assignment_operator_index(line: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }
        if is_quote_character(character) {
            quote = Some(character);
            continue;
        }
        if character != '=' {
            continue;
        }
        let before = line[..index].chars().next_back();
        let after = line[index.saturating_add(1)..].chars().next();
        if before.is_some_and(|value| matches!(value, '=' | '!' | '<' | '>'))
            || after.is_some_and(|value| value == '=' || value == '>')
        {
            continue;
        }
        return Some(index);
    }

    None
}

fn sdk_provider_initializer(value: &str) -> bool {
    sdk_provider_factory_expression(value)
}

fn statement_ranges(line: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0usize;
    for (index, character) in line.char_indices() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }
        if is_quote_character(character) {
            quote = Some(character);
            continue;
        }
        if character == ';' {
            ranges.push((start, index));
            start = index.saturating_add(character.len_utf8());
        }
    }
    ranges.push((start, line.len()));
    ranges
}

fn sdk_segment_ranges(line: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for (statement_start, statement_end) in statement_ranges(line) {
        let statement = &line[statement_start..statement_end];
        let mut quote = None;
        let mut escaped = false;
        let mut depth = 0usize;
        let mut start = statement_start;
        for (offset, character) in statement.char_indices() {
            if let Some(quote_character) = quote {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == quote_character {
                    quote = None;
                }
                continue;
            }
            if is_quote_character(character) {
                quote = Some(character);
                continue;
            }
            if character == '(' || character == '[' || character == '{' {
                depth = depth.saturating_add(1);
                continue;
            }
            if character == ')' || character == ']' || character == '}' {
                depth = depth.saturating_sub(1);
                continue;
            }
            if character == ',' && depth == 0 {
                let end = statement_start.saturating_add(offset);
                if assignment_operator_index(&line[start..end]).is_some() {
                    ranges.push((start, end));
                    start = end.saturating_add(character.len_utf8());
                }
            }
        }
        ranges.push((start, statement_end));
    }

    ranges
}

pub(super) fn sdk_flag_keys_for_line(
    line: &str,
    known_receivers: &mut BTreeMap<String, usize>,
    starting_depth: usize,
) -> Vec<String> {
    let mut keys = Vec::new();
    let line = template_interpolation_code(line);
    let line = line.as_str();
    for (start, end) in sdk_segment_ranges(line) {
        let statement = &line[start..end];
        for method in SDK_FLAG_METHODS {
            collect_sdk_quoted_arguments(statement, method, known_receivers, &mut keys);
        }
        if let Some((receiver, is_sdk_client)) = sdk_client_receiver_assignment(statement) {
            if is_sdk_client {
                let scope_depth = receiver_assignment_scope_depth(line, start, end, starting_depth);
                known_receivers.insert(receiver, scope_depth);
            } else {
                known_receivers.remove(&receiver);
            }
        }
    }

    keys
}

pub(super) fn sdk_pending_argument_index(
    line: &str,
    known_receivers: &BTreeMap<String, usize>,
) -> Option<usize> {
    let line = template_interpolation_code(line);
    let line = line.as_str();
    for method in SDK_FLAG_METHODS {
        let mut start = 0usize;
        while let Some(pattern_start) = find_sdk_pattern(line, method, start) {
            let value_start = pattern_start + method.len();
            start = value_start.saturating_add(1);
            if !sdk_receiver_allowed(&line[..pattern_start], known_receivers) {
                continue;
            }
            let argument_index = sdk_flag_argument_index(method);
            let remainder = &line[value_start..];
            if remainder.trim().is_empty() {
                return Some(argument_index);
            }
            if quoted_call_argument(remainder, argument_index).is_none() {
                if let Some(next_argument) =
                    sdk_next_pending_argument_index(remainder, argument_index)
                {
                    return Some(next_argument);
                }
            }
        }
    }

    None
}

pub(super) fn sdk_continued_flag_key(line: &str, target_argument: usize) -> Option<String> {
    quoted_call_argument(line, target_argument)
}

pub(super) fn sdk_next_pending_argument_index(
    line: &str,
    current_argument: usize,
) -> Option<usize> {
    let mut escaped = false;
    let mut quote = None;
    let mut depth = 0usize;
    let mut completed_arguments = 0usize;
    for character in line.chars() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }
        if is_quote_character(character) {
            quote = Some(character);
            continue;
        }
        if character == ')' && depth == 0 {
            return None;
        }
        if character == ',' && depth == 0 {
            completed_arguments = completed_arguments.saturating_add(1);
            continue;
        }
        if character == '(' {
            depth = depth.saturating_add(1);
            continue;
        }
        if character == ')' {
            depth = depth.saturating_sub(1);
        }
    }

    if completed_arguments > current_argument {
        None
    } else {
        Some(current_argument.saturating_sub(completed_arguments))
    }
}

pub(super) fn preprocessor_flag_keys(line: &str, language_id: &str) -> Vec<String> {
    if !matches!(language_id, "c" | "cpp" | "csharp") {
        return Vec::new();
    }
    let trimmed = line.trim_start();
    let remainder = if let Some(remainder) = trimmed.strip_prefix("#ifdef") {
        remainder
    } else if let Some(remainder) = trimmed.strip_prefix("#ifndef") {
        remainder
    } else if let Some(remainder) = trimmed.strip_prefix("#elif") {
        remainder
    } else if let Some(remainder) = trimmed.strip_prefix("#if") {
        remainder
    } else {
        return Vec::new();
    };

    let mut keys = Vec::new();
    collect_preprocessor_identifiers(remainder, &mut keys);

    keys
}

fn collect_preprocessor_identifiers(value: &str, keys: &mut Vec<String>) {
    let mut current = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            current.push(character);
            continue;
        }
        push_preprocessor_identifier(keys, &mut current);
    }
    push_preprocessor_identifier(keys, &mut current);
}

fn push_preprocessor_identifier(keys: &mut Vec<String>, current: &mut String) {
    if valid_preprocessor_key(current) {
        push_unique(keys, current.clone());
    }
    current.clear();
}

fn valid_preprocessor_key(key: &str) -> bool {
    valid_source_key(key)
        && key
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && !matches!(
            key,
            "defined" | "if" | "ifdef" | "ifndef" | "elif" | "true" | "false"
        )
}

fn collect_quoted_arguments(line: &str, pattern: &str, keys: &mut Vec<String>) {
    let mut start = 0usize;
    while let Some(pattern_start) = find_code_pattern(line, pattern, start) {
        let value_start = pattern_start + pattern.len();
        if let Some(key) = quoted_prefix(&line[value_start..]) {
            push_unique(keys, key);
        }
        start = value_start.saturating_add(1);
    }
}

fn collect_sdk_quoted_arguments(
    line: &str,
    pattern: &str,
    known_receivers: &BTreeMap<String, usize>,
    keys: &mut Vec<String>,
) {
    let mut start = 0usize;
    while let Some(pattern_start) = find_sdk_pattern(line, pattern, start) {
        let value_start = pattern_start + pattern.len();
        if sdk_receiver_allowed(&line[..pattern_start], known_receivers) {
            if let Some(key) =
                quoted_call_argument(&line[value_start..], sdk_flag_argument_index(pattern))
            {
                push_unique(keys, key);
            }
        }
        start = value_start.saturating_add(1);
    }
}

fn assignment_receiver_name(prefix: &str, allow_property: bool) -> Option<String> {
    let variable_prefix = prefix
        .rfind(':')
        .map_or(prefix, |type_separator| &prefix[..type_separator]);
    let binding_prefix = variable_prefix.split(',').next().unwrap_or(variable_prefix);
    let receiver = receiver_path_from_prefix(binding_prefix)?;
    if receiver.contains('.') && !allow_property {
        return None;
    }
    valid_receiver_path(&receiver).then_some(receiver)
}

fn sdk_receiver_allowed(prefix: &str, known_receivers: &BTreeMap<String, usize>) -> bool {
    if sdk_factory_prefix_allowed(prefix) {
        return true;
    }

    let Some(receiver) = receiver_path_from_prefix(prefix) else {
        return false;
    };
    let leaf_receiver = receiver
        .rsplit('.')
        .next()
        .unwrap_or(&receiver)
        .to_ascii_lowercase();

    valid_receiver_path(&receiver)
        && (leaf_receiver.contains("openfeature")
            || leaf_receiver.contains("featureflag")
            || leaf_receiver.contains("feature_flags")
            || leaf_receiver.contains("launchdarkly")
            || leaf_receiver.as_str() == "ldclient"
            || leaf_receiver.as_str() == "unleash"
            || leaf_receiver.as_str() == "flags"
            || leaf_receiver.as_str() == "flag"
            || leaf_receiver.as_str() == "toggles"
            || leaf_receiver.as_str() == "toggle"
            || known_receivers.contains_key(&receiver))
}

fn receiver_assignment_scope_depth(
    line: &str,
    segment_start: usize,
    segment_end: usize,
    starting_depth: usize,
) -> usize {
    let assignment_offset =
        assignment_operator_index(&line[segment_start..segment_end]).unwrap_or(0);
    let prefix_end = segment_start.saturating_add(assignment_offset);
    line_scope_depth(&line[..prefix_end], starting_depth)
}

fn line_scope_depth(prefix: &str, starting_depth: usize) -> usize {
    let mut depth = starting_depth;
    let mut quote = None;
    let mut escaped = false;
    for character in prefix.chars() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }
        if is_quote_character(character) {
            quote = Some(character);
            continue;
        }
        if character == '{' {
            depth = depth.saturating_add(1);
        } else if character == '}' {
            depth = depth.saturating_sub(1);
        }
    }

    depth
}

fn sdk_factory_prefix_allowed(prefix: &str) -> bool {
    immediate_receiver_expression(prefix).is_some_and(sdk_provider_factory_expression)
}

fn immediate_receiver_expression(prefix: &str) -> Option<&str> {
    let trimmed =
        prefix.trim_end_matches(|character: char| character.is_whitespace() || character == '?');
    if trimmed.ends_with(')') {
        return call_expression_receiver(trimmed);
    }

    let tail_start = trimmed
        .char_indices()
        .rev()
        .find_map(|(index, character)| {
            matches!(character, ';' | '{' | '[' | '=' | ',')
                .then_some(index.saturating_add(character.len_utf8()))
        })
        .unwrap_or(0);
    Some(trimmed[tail_start..].trim_start())
}

fn call_expression_receiver(value: &str) -> Option<&str> {
    let open_paren = matching_outer_call_open(value)?;
    let function_start = value[..open_paren]
        .char_indices()
        .rev()
        .find_map(|(index, character)| {
            (!(character.is_ascii_alphanumeric()
                || matches!(character, '_' | '.' | ':' | '<' | '>' | '-')))
            .then_some(index.saturating_add(character.len_utf8()))
        })
        .unwrap_or(0);
    Some(value[function_start..].trim_start())
}

fn matching_outer_call_open(value: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices().rev() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }
        if is_quote_character(character) {
            quote = Some(character);
            continue;
        }
        if character == ')' {
            depth = depth.saturating_add(1);
            continue;
        }
        if character == '(' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn sdk_provider_factory_expression(value: &str) -> bool {
    let value = unquoted_text(value).to_ascii_lowercase();
    let mut expression = value.trim().trim_end_matches('?').trim();
    while let Some(stripped) = expression.strip_prefix("await ") {
        expression = stripped.trim_start();
    }
    while outer_parentheses_wrap(expression) {
        expression = expression[1..expression.len().saturating_sub(1)].trim();
    }

    const SDK_PROVIDER_FACTORY_PATTERNS: &[&str] = &[
        "openfeature.getclient",
        "openfeature.getclient(",
        "openfeature::getclient",
        "openfeature::getclient(",
        "openfeatureapi.getclient",
        "openfeatureapi.getclient(",
        "openfeatureapi.getinstance().getclient",
        "openfeatureapi.getinstance().getclient(",
        "openfeature.api.instance.getclient",
        "openfeature.api.instance.getclient(",
        "openfeature.newclient",
        "openfeature.newclient(",
        "openfeature::newclient",
        "openfeature::newclient(",
        "ldclient.newclient",
        "ldclient.newclient(",
        "ldclient::newclient",
        "ldclient::newclient(",
        "ldclient.get",
        "ldclient.get(",
        "launchdarkly.init(",
        "launchdarkly.initialize(",
        "launchdarkly.start(",
        "new ldclient",
        "new launchdarkly",
        "unleash.init(",
        "unleash.initialize(",
        "unleash.start(",
    ];
    SDK_PROVIDER_FACTORY_PATTERNS.iter().any(|pattern| {
        if pattern.ends_with('(') || pattern.starts_with("new ") {
            expression.starts_with(pattern)
        } else {
            expression == *pattern
        }
    })
}

fn outer_parentheses_wrap(value: &str) -> bool {
    if !value.starts_with('(') || !value.ends_with(')') {
        return false;
    }
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }
        if is_quote_character(character) {
            quote = Some(character);
            continue;
        }
        if character == '(' {
            depth = depth.saturating_add(1);
            continue;
        }
        if character == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 && index < value.len().saturating_sub(1) {
                return false;
            }
        }
    }
    depth == 0
}

fn valid_receiver_name(receiver: &str) -> bool {
    !receiver.is_empty()
        && receiver
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && receiver
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn receiver_path_from_prefix(prefix: &str) -> Option<String> {
    let trimmed =
        prefix.trim_end_matches(|character: char| character.is_whitespace() || character == '?');
    let mut parts = Vec::new();
    let mut rest = trimmed;
    loop {
        let identifier = rest
            .chars()
            .rev()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        if !valid_receiver_name(&identifier) {
            break;
        }
        let identifier_start = rest.len().saturating_sub(identifier.len());
        parts.push(identifier);
        let before = rest[..identifier_start]
            .trim_end_matches(|character: char| character.is_whitespace() || character == '?');
        if !before.ends_with('.') {
            break;
        }
        rest = &before[..before.len().saturating_sub(1)];
    }
    if parts.is_empty() {
        return None;
    }
    parts.reverse();
    Some(parts.join("."))
}

fn valid_receiver_path(receiver: &str) -> bool {
    receiver.split('.').all(valid_receiver_name)
}

fn quoted_call_argument(value: &str, target_argument: usize) -> Option<String> {
    let (argument_start, argument_end) = call_argument_range(value, target_argument)?;
    quoted_argument(&value[argument_start..argument_end])
}

fn call_argument_range(value: &str, target_argument: usize) -> Option<(usize, usize)> {
    let mut escaped = false;
    let mut quote = None;
    let mut depth = 0usize;
    let mut argument_index = 0usize;
    let mut argument_start = 0usize;
    for (index, character) in value.char_indices() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }

        if character == ')' && depth == 0 {
            return (argument_index == target_argument).then_some((argument_start, index));
        }
        if character == ',' && depth == 0 {
            if argument_index == target_argument {
                return Some((argument_start, index));
            }
            argument_index = argument_index.saturating_add(1);
            argument_start = index.saturating_add(character.len_utf8());
            continue;
        }
        if character == '(' {
            depth = depth.saturating_add(1);
            continue;
        }
        if character == ')' {
            depth = depth.saturating_sub(1);
            continue;
        }
        if is_quote_character(character) {
            quote = Some(character);
        }
    }

    None
}

fn quoted_argument(value: &str) -> Option<String> {
    let value = value.trim();
    let mut chars = value.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut escaped = false;
    for (index, character) in value[quote.len_utf8()..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote {
            let end = quote.len_utf8().saturating_add(index);
            let key = &value[quote.len_utf8()..end];
            return valid_source_key(key).then(|| key.to_owned());
        }
    }

    None
}

fn collect_bracket_keys(line: &str, pattern: &str, keys: &mut Vec<String>) {
    collect_quoted_arguments(line, pattern, keys);
}

fn collect_bounded_quoted_arguments(line: &str, pattern: &str, keys: &mut Vec<String>) {
    let mut start = 0usize;
    while let Some(pattern_start) = find_code_pattern(line, pattern, start) {
        let value_start = pattern_start + pattern.len();
        if token_boundary_before(line, pattern_start) {
            if let Some(key) = quoted_prefix(&line[value_start..]) {
                push_unique(keys, key);
            }
        }
        start = value_start.saturating_add(1);
    }
}

fn collect_dotted_members(line: &str, pattern: &str, keys: &mut Vec<String>) {
    let mut start = 0usize;
    while let Some(pattern_start) = find_code_pattern(line, pattern, start) {
        let member_start = pattern_start + pattern.len();
        let member = line[member_start..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if valid_source_key(&member) {
            push_unique(keys, member.clone());
        }
        start = member_start.saturating_add(member.len().max(1));
    }
}

fn find_code_pattern(line: &str, pattern: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0usize;
    while index < line.len() {
        let rest = &line[index..];
        let character = rest.chars().next()?;
        if let Some(quote_character) = quote {
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

        if index >= start && rest.starts_with(pattern) {
            return Some(index);
        }
        if is_string_quote_character(character) {
            quote = Some(character);
        }
        index = index.saturating_add(character.len_utf8());
    }

    None
}

fn find_sdk_pattern(line: &str, pattern: &str, start: usize) -> Option<usize> {
    find_pattern_with_quotes(line, pattern, start, is_quote_character)
}

fn find_pattern_with_quotes(
    line: &str,
    pattern: &str,
    start: usize,
    quote_predicate: fn(char) -> bool,
) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0usize;
    while index < line.len() {
        let rest = &line[index..];
        let character = rest.chars().next()?;
        if let Some(quote_character) = quote {
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

        if index >= start && rest.starts_with(pattern) {
            return Some(index);
        }
        if quote_predicate(character) {
            quote = Some(character);
        }
        index = index.saturating_add(character.len_utf8());
    }

    None
}

fn quoted_prefix(value: &str) -> Option<String> {
    let value = value.trim_start();
    let mut chars = value.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = value[1..].find(quote)?;
    let key = &value[1..1 + end];
    valid_source_key(key).then(|| key.to_owned())
}

fn is_quote_character(character: char) -> bool {
    matches!(character, '"' | '\'' | '`')
}

fn is_string_quote_character(character: char) -> bool {
    matches!(character, '"' | '\'')
}

fn token_boundary_before(line: &str, start: usize) -> bool {
    line[..start]
        .chars()
        .next_back()
        .is_none_or(|character| !is_source_token_character(character))
}

fn is_source_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn unquoted_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut quote = None;
    let mut escaped = false;
    for character in value.chars() {
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            continue;
        }
        if is_quote_character(character) {
            quote = Some(character);
            continue;
        }
        output.push(character);
    }
    output
}

fn valid_source_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 160
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
        })
}

pub(super) fn usage_edge_kind(line: &str) -> &'static str {
    if line_looks_conditional(line) {
        "guards_code"
    } else {
        "reads_config"
    }
}

fn line_looks_conditional(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("if ")
        || trimmed.starts_with("if(")
        || trimmed.starts_with("elif ")
        || trimmed.starts_with("else if")
        || trimmed.starts_with("while ")
        || trimmed.contains(" if ")
        || trimmed.contains(" ? ")
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}
