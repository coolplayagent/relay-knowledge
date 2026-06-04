pub(super) fn specific_call_identity_leaf(leaf_name: &str) -> bool {
    leaf_name.len() >= 8 || leaf_name.contains('_') || has_case_boundary(leaf_name)
}

pub(super) fn has_case_boundary(value: &str) -> bool {
    let mut previous: Option<char> = None;
    for character in value.chars() {
        if character.is_ascii_uppercase()
            && previous.is_some_and(|previous| previous.is_ascii_lowercase())
        {
            return true;
        }
        previous = Some(character);
    }

    false
}
