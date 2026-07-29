    fn long_options_from_source(source: &str) -> BTreeSet<String> {
        source
            .split('"')
            .filter_map(|item| item.strip_prefix("--"))
            .map(|item| {
                let name = item
                    .split(|ch: char| ch == '=' || ch.is_whitespace())
                    .next()
                    .unwrap_or_default();
                format!("--{name}")
            })
            .filter(|option| option.len() > 2 && !option.contains('{'))
            .collect()
    }
