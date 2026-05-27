use std::collections::{HashMap, HashSet};

pub(super) struct MacroFunctionDefinition {
    pub(super) parameters: Vec<String>,
    pub(super) replacement: String,
}

pub(super) enum LocalFunctionMacroDefinition {
    Function(MacroFunctionDefinition),
    ActiveNonFunction,
    Unavailable,
    Missing,
}

pub(super) fn local_function_macro_definition(
    content: &str,
    macro_name: &str,
    limit_byte: usize,
) -> LocalFunctionMacroDefinition {
    let search_end = limit_byte.min(content.len());
    let mut active_macros = HashMap::new();
    let mut branches = Vec::new();
    let mut latest = None;
    let mut active_non_function = false;
    let mut seen_unavailable_macro = false;
    let mut logical_line = String::new();
    for line in content[..search_end].lines() {
        if !logical_line.is_empty() {
            append_preprocessor_logical_line(&mut logical_line, line);
            if !line_continues_preprocessor_directive(line) {
                apply_preprocessor_logical_line(
                    &logical_line,
                    macro_name,
                    &mut active_macros,
                    &mut branches,
                    &mut latest,
                    &mut active_non_function,
                    &mut seen_unavailable_macro,
                );
                logical_line.clear();
            }
            continue;
        }
        let trimmed_start = line.trim_start();
        if preprocessor_directive(trimmed_start).is_none() {
            continue;
        }
        append_preprocessor_logical_line(&mut logical_line, line);
        if !line_continues_preprocessor_directive(line) {
            apply_preprocessor_logical_line(
                &logical_line,
                macro_name,
                &mut active_macros,
                &mut branches,
                &mut latest,
                &mut active_non_function,
                &mut seen_unavailable_macro,
            );
            logical_line.clear();
        }
    }
    if !logical_line.is_empty() {
        apply_preprocessor_logical_line(
            &logical_line,
            macro_name,
            &mut active_macros,
            &mut branches,
            &mut latest,
            &mut active_non_function,
            &mut seen_unavailable_macro,
        );
    }

    match (latest, active_non_function, seen_unavailable_macro) {
        (Some(definition), _, _) => LocalFunctionMacroDefinition::Function(definition),
        (None, true, _) => LocalFunctionMacroDefinition::ActiveNonFunction,
        (None, false, true) => LocalFunctionMacroDefinition::Unavailable,
        (None, false, false) => LocalFunctionMacroDefinition::Missing,
    }
}

struct PreprocessorDirective<'a> {
    keyword: &'a str,
    rest: &'a str,
}

fn preprocessor_directive(line: &str) -> Option<PreprocessorDirective<'_>> {
    let directive = line.strip_prefix('#')?.trim_start();
    let keyword_end = directive
        .find(|character: char| !c_identifier_char(character))
        .unwrap_or(directive.len());
    let keyword = &directive[..keyword_end];
    if keyword.is_empty() {
        return None;
    }

    Some(PreprocessorDirective {
        keyword,
        rest: directive[keyword_end..].trim_start(),
    })
}

fn apply_preprocessor_logical_line(
    line: &str,
    macro_name: &str,
    active_macros: &mut HashMap<String, ActiveMacroDefinition>,
    branches: &mut Vec<PreprocessorBranch>,
    latest: &mut Option<MacroFunctionDefinition>,
    active_non_function: &mut bool,
    seen_unavailable_macro: &mut bool,
) {
    let Some(directive) = preprocessor_directive(line.trim_start()) else {
        return;
    };
    if update_preprocessor_branch(&directive, active_macros, branches) {
        return;
    }
    if !preprocessor_branches_active(branches) {
        *seen_unavailable_macro |= directive.keyword == "define"
            && directive_identifier(directive.rest).is_some_and(|name| name == macro_name);
        return;
    }

    match directive.keyword {
        "define" => {
            apply_define_logical_line(line, macro_name, active_macros, latest, active_non_function);
        }
        "undef" => {
            if let Some(name) = directive_identifier(directive.rest) {
                active_macros.remove(name);
                if name == macro_name {
                    *seen_unavailable_macro = true;
                    *active_non_function = false;
                    *latest = None;
                }
            }
        }
        _ => {}
    }
}

fn apply_define_logical_line(
    line: &str,
    macro_name: &str,
    active_macros: &mut HashMap<String, ActiveMacroDefinition>,
    latest: &mut Option<MacroFunctionDefinition>,
    active_non_function: &mut bool,
) {
    let Some((name, active_macro)) = parse_active_macro_definition_line(line) else {
        return;
    };
    active_macros.insert(name.clone(), active_macro);
    if name == macro_name {
        *latest = parse_function_macro_definition_line(line, macro_name);
        *active_non_function = latest.is_none();
    }
}

struct ActiveMacroDefinition {
    replacement: String,
    function_like: bool,
}

struct PreprocessorBranch {
    parent_active: bool,
    branch_active: bool,
    branch_taken: bool,
}

fn update_preprocessor_branch(
    directive: &PreprocessorDirective<'_>,
    active_macros: &HashMap<String, ActiveMacroDefinition>,
    branches: &mut Vec<PreprocessorBranch>,
) -> bool {
    match directive.keyword {
        "if" => {
            push_preprocessor_branch(
                branches,
                evaluate_if_condition(directive.rest, active_macros),
            );
            true
        }
        "ifdef" => {
            let active = directive_identifier(directive.rest)
                .is_some_and(|name| active_macros.contains_key(name));
            push_preprocessor_branch(branches, active);
            true
        }
        "ifndef" => {
            let active = directive_identifier(directive.rest)
                .is_none_or(|name| !active_macros.contains_key(name));
            push_preprocessor_branch(branches, active);
            true
        }
        "elif" => {
            let active = evaluate_if_condition(directive.rest, active_macros);
            apply_preprocessor_elif(branches, active);
            true
        }
        "else" => {
            apply_preprocessor_else(branches);
            true
        }
        "endif" => {
            branches.pop();
            true
        }
        _ => false,
    }
}

fn push_preprocessor_branch(branches: &mut Vec<PreprocessorBranch>, condition_active: bool) {
    let parent_active = preprocessor_branches_active(branches);
    let branch_active = parent_active && condition_active;
    branches.push(PreprocessorBranch {
        parent_active,
        branch_active,
        branch_taken: branch_active,
    });
}

fn apply_preprocessor_elif(branches: &mut [PreprocessorBranch], condition_active: bool) {
    let Some(branch) = branches.last_mut() else {
        return;
    };
    branch.branch_active = branch.parent_active && !branch.branch_taken && condition_active;
    branch.branch_taken |= branch.branch_active;
}

fn apply_preprocessor_else(branches: &mut [PreprocessorBranch]) {
    let Some(branch) = branches.last_mut() else {
        return;
    };
    branch.branch_active = branch.parent_active && !branch.branch_taken;
    branch.branch_taken = true;
}

fn preprocessor_branches_active(branches: &[PreprocessorBranch]) -> bool {
    branches.last().is_none_or(|branch| branch.branch_active)
}

fn evaluate_if_condition(
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

fn directive_identifier(text: &str) -> Option<&str> {
    let end = text
        .find(|character: char| !c_identifier_char(character))
        .unwrap_or(text.len());
    let name = &text[..end];
    if name.is_empty() {
        return None;
    }

    Some(name)
}

fn append_preprocessor_logical_line(logical_line: &mut String, line: &str) {
    if !logical_line.is_empty() {
        logical_line.push(' ');
    }
    let segment = line
        .trim_end()
        .strip_suffix('\\')
        .unwrap_or_else(|| line.trim_end())
        .trim();
    logical_line.push_str(segment);
}

fn line_continues_preprocessor_directive(line: &str) -> bool {
    line.trim_end().ends_with('\\')
}

fn parse_active_macro_definition_line(line: &str) -> Option<(String, ActiveMacroDefinition)> {
    let directive = preprocessor_directive(line.trim_start())?;
    if directive.keyword != "define" {
        return None;
    }
    let name = directive_identifier(directive.rest)?;
    let after_name = &directive.rest[name.len()..];
    let function_like = after_name.starts_with('(');
    let replacement = if function_like {
        let parameters_end = closing_parenthesis_index(after_name)?;
        after_name[parameters_end + 1..].trim()
    } else {
        after_name.trim()
    };

    Some((
        name.to_owned(),
        ActiveMacroDefinition {
            replacement: replacement.to_owned(),
            function_like,
        },
    ))
}

fn parse_function_macro_definition_line(
    line: &str,
    macro_name: &str,
) -> Option<MacroFunctionDefinition> {
    let directive = preprocessor_directive(line.trim_start())?;
    if directive.keyword != "define" {
        return None;
    }
    let after_name = directive.rest.strip_prefix(macro_name)?;
    if !after_name.starts_with('(') {
        return None;
    }
    let parameters_end = closing_parenthesis_index(after_name)?;
    let replacement = after_name[parameters_end + 1..].trim();
    if replacement.is_empty() {
        return None;
    }
    let parameters = after_name[1..parameters_end]
        .split(',')
        .map(str::trim)
        .filter(|parameter| !parameter.is_empty() && *parameter != "...")
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if parameters.is_empty() {
        return None;
    }

    Some(MacroFunctionDefinition {
        parameters,
        replacement: replacement.to_owned(),
    })
}

fn closing_parenthesis_index(text: &str) -> Option<usize> {
    if !text.starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn c_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn c_identifier_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_macro_lookup_accepts_spaced_define_directives() {
        let content = "# define KONG_ACCESS_PHASE(name) \\\n    static ngx_int_t name(ngx_http_request_t *request)\n";
        let LocalFunctionMacroDefinition::Function(definition) =
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
        else {
            panic!("spaced define directive should be visible");
        };

        assert_eq!(definition.parameters, ["name"]);
        assert!(definition.replacement.contains("name("));
    }

    #[test]
    fn local_macro_lookup_respects_undef() {
        let content = "\
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#undef KONG_ACCESS_PHASE
";

        assert!(matches!(
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len()),
            LocalFunctionMacroDefinition::Unavailable
        ));
    }

    #[test]
    fn local_macro_lookup_ignores_inactive_branches() {
        let content = "\
#if 0
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
#if FEATURE_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
#ifdef NEVER_DEFINED
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";

        assert!(matches!(
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len()),
            LocalFunctionMacroDefinition::Unavailable
        ));
    }

    #[test]
    fn local_macro_lookup_evaluates_numeric_macro_conditions() {
        let disabled = "\
#define FEATURE_FLAG 0
#if FEATURE_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";

        assert!(matches!(
            local_function_macro_definition(disabled, "KONG_ACCESS_PHASE", disabled.len()),
            LocalFunctionMacroDefinition::Unavailable
        ));

        let enabled_by_negation = "\
#define FEATURE_FLAG 0
#if !FEATURE_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
        let LocalFunctionMacroDefinition::Function(definition) = local_function_macro_definition(
            enabled_by_negation,
            "KONG_ACCESS_PHASE",
            enabled_by_negation.len(),
        ) else {
            panic!("numeric macro expansion should make !FEATURE_FLAG active");
        };

        assert_eq!(definition.parameters, ["name"]);
    }

    #[test]
    fn local_macro_lookup_requires_complete_defined_conditions() {
        let missing_rhs = "\
#define ENABLE_A 1
#if defined(ENABLE_A) && defined(ENABLE_B)
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";

        assert!(matches!(
            local_function_macro_definition(missing_rhs, "KONG_ACCESS_PHASE", missing_rhs.len()),
            LocalFunctionMacroDefinition::Unavailable
        ));

        let complete_condition = "\
#define ENABLE_A 1
#define ENABLE_B 1
#if defined(ENABLE_A) && defined(ENABLE_B)
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
        let LocalFunctionMacroDefinition::Function(definition) = local_function_macro_definition(
            complete_condition,
            "KONG_ACCESS_PHASE",
            complete_condition.len(),
        ) else {
            panic!("complete defined conjunction should activate the branch");
        };

        assert_eq!(definition.parameters, ["name"]);
    }

    #[test]
    fn local_macro_lookup_parses_standard_numeric_constants() {
        let content = "\
#if 1U && 0x1 && (1) && 1 /* comment */
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
        let LocalFunctionMacroDefinition::Function(definition) =
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
        else {
            panic!("standard numeric constants should activate the branch");
        };

        assert_eq!(definition.parameters, ["name"]);
    }

    #[test]
    fn local_macro_lookup_evaluates_comparison_conditions() {
        let content = "\
#define FEATURE_FLAG 1
#define VERSION 3
#if FEATURE_FLAG == 1 && VERSION >= 2
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
        let LocalFunctionMacroDefinition::Function(definition) =
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
        else {
            panic!("comparison conditions should activate matching branches");
        };

        assert_eq!(definition.parameters, ["name"]);

        let inactive = "\
#define VERSION 3
#if VERSION < 2
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
        assert!(matches!(
            local_function_macro_definition(inactive, "KONG_ACCESS_PHASE", inactive.len()),
            LocalFunctionMacroDefinition::Unavailable
        ));
    }

    #[test]
    fn local_macro_lookup_joins_continued_if_conditions() {
        let content = "\
#define FEATURE_FLAG 1
#define EXTRA_FLAG 1
#if FEATURE_FLAG \\
    && EXTRA_FLAG
#define KONG_ACCESS_PHASE(name) static ngx_int_t name(ngx_http_request_t *request)
#endif
";
        let LocalFunctionMacroDefinition::Function(definition) =
            local_function_macro_definition(content, "KONG_ACCESS_PHASE", content.len())
        else {
            panic!("continued #if conditions should activate matching branches");
        };

        assert_eq!(definition.parameters, ["name"]);
    }
}
