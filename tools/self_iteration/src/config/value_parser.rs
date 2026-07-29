struct Parser {
    args: Vec<String>,
    index: usize,
}

impl Parser {
    fn new(args: Vec<String>) -> Self {
        Self { args, index: 0 }
    }

    fn take_mode(&mut self) -> Option<Mode> {
        let mode = self.args.first().and_then(|arg| Mode::parse(arg));
        if mode.is_some() {
            self.index = 1;
        }
        mode
    }

    fn next(&mut self) -> Option<String> {
        let next = self.args.get(self.index).cloned();
        if next.is_some() {
            self.index += 1;
        }
        next
    }

    fn value(&mut self, name: &str) -> Result<String, String> {
        let value = self
            .args
            .get(self.index)
            .ok_or_else(|| format!("missing value for {name}"))?
            .clone();
        self.index += 1;
        Ok(value)
    }
}

fn profile(value: String) -> Result<String, String> {
    if matches!(value.as_str(), "fast" | "full" | "smoke" | "exhaustive") {
        Ok(value)
    } else {
        Err(format!("invalid profile: {value}"))
    }
}

fn codex_reasoning_effort(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "low" | "medium" | "high" | "xhigh" => Ok(normalized),
        _ => Err(format!("invalid codex reasoning effort: {value}")),
    }
}

fn positive_u64(value: &str, name: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("invalid value for {name}: {value}"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn non_empty_value(value: &str, name: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    Ok(trimmed.to_owned())
}

fn research_slug(value: &str) -> Result<String, String> {
    let slug = non_empty_value(value, "--research-slug")?;
    if !slug
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(
            "--research-slug may contain only lowercase ASCII letters, digits, '.', '-', or '_'"
                .to_owned(),
        );
    }
    Ok(slug)
}

fn research_date(value: &str) -> Result<String, String> {
    let date = non_empty_value(value, "--research-date")?;
    let bytes = date.as_bytes();
    let valid = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit());
    if !valid {
        return Err("--research-date must use YYYY-MM-DD".to_owned());
    }
    Ok(date)
}

fn positive_usize(value: &str, name: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid value for {name}: {value}"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn suffix<'a>(value: &'a str, prefix: &str) -> &'a str {
    value.strip_prefix(prefix).unwrap_or(value)
}

fn default_workspace() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|error| error.to_string())
}
