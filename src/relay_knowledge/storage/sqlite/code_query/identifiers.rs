pub(super) fn identifier_terms_equivalent(candidate: &str, token: &str) -> bool {
    if candidate.eq_ignore_ascii_case(token) {
        return true;
    }
    let candidate_singular = singular_identifier_term(candidate);
    if candidate_singular
        .as_deref()
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(token))
    {
        return true;
    }
    let Some(token_singular) = singular_identifier_term(token) else {
        return false;
    };

    candidate.eq_ignore_ascii_case(&token_singular)
        || candidate_singular
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&token_singular))
}

fn singular_identifier_term(term: &str) -> Option<String> {
    if !term.is_ascii() {
        return None;
    }
    let lower = term.to_ascii_lowercase();
    if lower.len() < 4
        || !lower
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return None;
    }
    if lower == "series" || lower == "species" {
        return None;
    }
    if lower.ends_with("ies") && lower.len() > 4 {
        let mut singular = lower[..lower.len() - 3].to_owned();
        singular.push('y');
        Some(singular)
    } else if lower.ends_with('s')
        && !lower.ends_with("ss")
        && !lower.ends_with("us")
        && !lower.ends_with("is")
    {
        Some(lower[..lower.len() - 1].to_owned())
    } else {
        None
    }
}
