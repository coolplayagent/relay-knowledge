pub(super) fn normalized_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn identifier_ranges<'a>(
    line: &'a str,
    name: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    line.match_indices(name).filter_map(|(start, _)| {
        let end = start + name.len();
        let has_start_boundary = line.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|character| !identifier_char(character))
        });
        let has_end_boundary = line.get(end..).is_some_and(|suffix| {
            suffix
                .chars()
                .next()
                .is_none_or(|character| !identifier_char(character))
        });
        (has_start_boundary && has_end_boundary).then_some((start, end))
    })
}

fn identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

#[cfg(test)]
#[path = "identifier_text_tests.rs"]
mod tests;
