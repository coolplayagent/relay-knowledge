use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Loop,
    Once,
    Evaluate,
    Chart,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "loop" => Some(Self::Loop),
            "once" => Some(Self::Once),
            "evaluate" => Some(Self::Evaluate),
            "chart" => Some(Self::Chart),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jobs {
    Auto,
    Fixed(usize),
}

impl Jobs {
    fn parse(value: &str) -> Result<Self, String> {
        if value == "auto" {
            return Ok(Self::Auto);
        }
        let parsed = value
            .parse::<usize>()
            .map_err(|_| format!("invalid job value: {value}"))?;
        if parsed == 0 {
            return Err("job value must be greater than zero".to_owned());
        }
        Ok(Self::Fixed(parsed))
    }

    fn resolve(self, default: usize) -> usize {
        match self {
            Self::Auto => default.max(1),
            Self::Fixed(value) => value.max(1),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Auto => "auto".to_owned(),
            Self::Fixed(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub mode: Mode,
    pub workspace: PathBuf,
    pub yolo: bool,
    pub model: Option<String>,
    pub codex_profile: Option<String>,
    pub codex_path: Option<String>,
    pub codex_timeout_seconds: u64,
    pub command_timeout_seconds: u64,
    pub profile: String,
    pub max_iterations: Option<usize>,
    pub stop_after_accepted: Option<usize>,
    pub sleep_seconds: u64,
    pub commit_message: Option<String>,
    pub dry_run_codex: bool,
    pub keep_workdirs: bool,
    pub use_current_candidate: bool,
    pub fail_fast: bool,
    pub jobs: Jobs,
    pub repo_jobs: Jobs,
    pub query_jobs: Jobs,
}

impl Config {
    pub fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut parser = Parser::new(args);
        let mode = parser.take_mode().unwrap_or(Mode::Loop);
        let mut config = Self {
            mode,
            workspace: default_workspace()?,
            yolo: false,
            model: None,
            codex_profile: None,
            codex_path: None,
            codex_timeout_seconds: 3600,
            command_timeout_seconds: 900,
            profile: "full".to_owned(),
            max_iterations: None,
            stop_after_accepted: None,
            sleep_seconds: 5,
            commit_message: None,
            dry_run_codex: false,
            keep_workdirs: false,
            use_current_candidate: false,
            fail_fast: false,
            jobs: Jobs::Auto,
            repo_jobs: Jobs::Auto,
            query_jobs: Jobs::Auto,
        };
        while let Some(arg) = parser.next() {
            match arg.as_str() {
                "--workspace" => config.workspace = PathBuf::from(parser.value("--workspace")?),
                "--yolo" => config.yolo = true,
                "--model" => config.model = Some(parser.value("--model")?),
                "--codex-profile" => config.codex_profile = Some(parser.value("--codex-profile")?),
                "--codex-path" => config.codex_path = Some(parser.value("--codex-path")?),
                "--codex-timeout-seconds" => {
                    config.codex_timeout_seconds = positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--command-timeout-seconds" => {
                    config.command_timeout_seconds = positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--profile" => config.profile = profile(parser.value("--profile")?)?,
                "--max-iterations" => {
                    config.max_iterations = Some(positive_usize(&parser.value(&arg)?, &arg)?);
                }
                "--stop-after-accepted" => {
                    config.stop_after_accepted = Some(positive_usize(&parser.value(&arg)?, &arg)?);
                }
                "--sleep-seconds" => {
                    config.sleep_seconds = positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--commit-message" => {
                    config.commit_message = Some(parser.value("--commit-message")?)
                }
                "--dry-run-codex" => config.dry_run_codex = true,
                "--keep-workdirs" => config.keep_workdirs = true,
                "--use-current-candidate" => config.use_current_candidate = true,
                "--fail-fast" => config.fail_fast = true,
                "--jobs" => config.jobs = Jobs::parse(&parser.value("--jobs")?)?,
                "--repo-jobs" => config.repo_jobs = Jobs::parse(&parser.value("--repo-jobs")?)?,
                "--query-jobs" => config.query_jobs = Jobs::parse(&parser.value("--query-jobs")?)?,
                other if other.starts_with("--workspace=") => {
                    config.workspace = PathBuf::from(suffix(other, "--workspace="));
                }
                other if other.starts_with("--profile=") => {
                    config.profile = profile(suffix(other, "--profile=").to_owned())?;
                }
                other if other.starts_with("--jobs=") => {
                    config.jobs = Jobs::parse(suffix(other, "--jobs="))?;
                }
                other if other.starts_with("--repo-jobs=") => {
                    config.repo_jobs = Jobs::parse(suffix(other, "--repo-jobs="))?;
                }
                other if other.starts_with("--query-jobs=") => {
                    config.query_jobs = Jobs::parse(suffix(other, "--query-jobs="))?;
                }
                other => return Err(format!("unexpected argument: {other}")),
            }
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobPlan {
    pub global: usize,
    pub repositories: usize,
    pub queries: usize,
}

impl JobPlan {
    pub fn resolve(config: &Config) -> Self {
        let cores = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(2);
        let env_jobs = std::env::var("RELAY_KNOWLEDGE_SELF_ITERATION_JOBS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);
        let global_default = env_jobs.unwrap_or_else(|| cores.clamp(2, 8));
        Self {
            global: config.jobs.resolve(global_default),
            repositories: config.repo_jobs.resolve((cores / 2).clamp(1, 4)),
            queries: config.query_jobs.resolve(cores.clamp(2, 12)),
        }
    }
}

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
    if matches!(value.as_str(), "full" | "smoke" | "exhaustive") {
        Ok(value)
    } else {
        Err(format!("invalid profile: {value}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_evaluate_with_jobs() {
        let config = Config::parse(vec![
            "evaluate".to_owned(),
            "--profile".to_owned(),
            "smoke".to_owned(),
            "--jobs=2".to_owned(),
            "--repo-jobs".to_owned(),
            "1".to_owned(),
        ])
        .expect("config should parse");

        assert_eq!(config.mode, Mode::Evaluate);
        assert_eq!(config.profile, "smoke");
        assert_eq!(config.jobs, Jobs::Fixed(2));
        assert_eq!(config.repo_jobs, Jobs::Fixed(1));
    }
}
