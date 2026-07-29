fn unattended_config(
    config: &Config,
    profile: &str,
    category: EvaluationCategory,
    codex_timeout_seconds: u64,
) -> Config {
    let mut next = config.clone();
    next.profile = profile.to_owned();
    next.categories = Some(CategorySet::single(category));
    next.codex_timeout_seconds = codex_timeout_seconds.max(1);
    next.use_current_candidate = false;
    next
}
