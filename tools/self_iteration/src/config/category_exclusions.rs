fn apply_category_exclusions(
    config: &mut Config,
    excluded_categories: Option<CategorySet>,
) -> Result<(), String> {
    let Some(excluded) = excluded_categories else {
        return Ok(());
    };
    let mut selected = config.categories.clone().unwrap_or_else(CategorySet::all);
    selected.remove_all(&excluded);
    if selected.is_empty() {
        return Err("--exclude-categories removed all selected categories".to_owned());
    }
    config.categories = Some(selected);
    Ok(())
}
