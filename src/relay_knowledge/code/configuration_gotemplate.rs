use super::valid_config_character;

pub(super) fn syntax(content: &str) -> bool {
    content.contains("{{")
}

pub(super) fn actions<'a>(line: &'a str, action: &str) -> Vec<&'a str> {
    let mut actions = Vec::new();
    for part in line.split("{{").skip(1) {
        let part = part
            .split("}}")
            .next()
            .unwrap_or(part)
            .trim_start_matches('-')
            .trim_start();
        if let Some(rest) = part.strip_prefix(action) {
            if rest
                .chars()
                .next()
                .is_none_or(|character| !valid_config_character(character))
            {
                actions.push(rest.trim_start().trim_end_matches('-').trim_end());
            }
        }
    }

    actions
}
