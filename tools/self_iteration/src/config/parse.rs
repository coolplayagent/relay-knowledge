use std::path::PathBuf;

use super::{
    DEFAULT_CODEX_MODEL, DEFAULT_CODEX_REASONING_EFFORT,
    categories::CategorySet,
    category_exclusions::apply_category_exclusions,
    jobs::Jobs,
    mode::{Mode, Strategy},
    model::Config,
    value_parser::{
        Parser, codex_reasoning_effort, default_workspace, non_empty_value, positive_u64,
        positive_usize, profile, research_date, research_slug, suffix,
    },
};

impl Config {
    pub fn selected_category_labels(&self) -> Vec<&'static str> {
        self.categories
            .as_ref()
            .map(CategorySet::labels)
            .unwrap_or_default()
    }

    pub fn category_focus_key(&self) -> Option<String> {
        self.categories.as_ref().map(CategorySet::focus_key)
    }

    pub fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut parser = Parser::new(args);
        let mode = parser.take_mode().unwrap_or(Mode::Loop);
        let mut excluded_categories: Option<CategorySet> = None;
        let mut config = Self {
            mode,
            strategy: Strategy::Single,
            workspace: default_workspace()?,
            yolo: false,
            model: Some(DEFAULT_CODEX_MODEL.to_owned()),
            codex_reasoning_effort: DEFAULT_CODEX_REASONING_EFFORT.to_owned(),
            codex_profile: None,
            codex_path: None,
            codex_timeout_seconds: 3600,
            command_timeout_seconds: 900,
            profile: "fast".to_owned(),
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
            categories: None,
            max_wall_clock_hours: 36,
            explore_timeout_seconds: 900,
            macro_explore_timeout_seconds: 2700,
            max_explore_attempts_per_cycle: 3,
            max_consecutive_empty_candidates: 8,
            max_consecutive_promotion_failures: 10,
            macro_after_competitive_failures: 4,
            macro_after_empty_candidates: 6,
            cycle_sleep_seconds: 120,
            cooldown_after_accept_seconds: 300,
            cooldown_after_timeout_seconds: 900,
            deep_check_interval_accepts: 6,
            deep_check_interval_hours: 12,
            research_topic: "relay-knowledge research iteration".to_owned(),
            research_slug: "research-iteration".to_owned(),
            research_date: "YYYY-MM-DD".to_owned(),
        };
        while let Some(arg) = parser.next() {
            match arg.as_str() {
                "--workspace" => config.workspace = PathBuf::from(parser.value("--workspace")?),
                "--strategy" => config.strategy = Strategy::parse(&parser.value("--strategy")?)?,
                "--yolo" => config.yolo = true,
                "--model" => config.model = Some(parser.value("--model")?),
                "--codex-reasoning-effort" => {
                    config.codex_reasoning_effort =
                        codex_reasoning_effort(&parser.value("--codex-reasoning-effort")?)?;
                }
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
                    let value = positive_u64(&parser.value(&arg)?, &arg)?;
                    config.sleep_seconds = value;
                    config.cycle_sleep_seconds = value;
                }
                "--cycle-sleep-seconds" => {
                    config.cycle_sleep_seconds = positive_u64(&parser.value(&arg)?, &arg)?;
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
                "--categories" => {
                    config.categories = Some(CategorySet::parse(&parser.value("--categories")?)?);
                }
                "--exclude-categories" => {
                    excluded_categories =
                        Some(CategorySet::parse(&parser.value("--exclude-categories")?)?);
                }
                "--max-wall-clock-hours" => {
                    config.max_wall_clock_hours = positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--explore-timeout-seconds" => {
                    config.explore_timeout_seconds = positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--macro-explore-timeout-seconds" => {
                    config.macro_explore_timeout_seconds =
                        positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--max-explore-attempts-per-cycle" => {
                    config.max_explore_attempts_per_cycle =
                        positive_usize(&parser.value(&arg)?, &arg)?;
                }
                "--max-consecutive-empty-candidates" => {
                    config.max_consecutive_empty_candidates =
                        positive_usize(&parser.value(&arg)?, &arg)?;
                }
                "--max-consecutive-promotion-failures" => {
                    config.max_consecutive_promotion_failures =
                        positive_usize(&parser.value(&arg)?, &arg)?;
                }
                "--macro-after-competitive-failures" => {
                    config.macro_after_competitive_failures =
                        positive_usize(&parser.value(&arg)?, &arg)?;
                }
                "--macro-after-empty-candidates" => {
                    config.macro_after_empty_candidates =
                        positive_usize(&parser.value(&arg)?, &arg)?;
                }
                "--cooldown-after-accept-seconds" => {
                    config.cooldown_after_accept_seconds =
                        positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--cooldown-after-timeout-seconds" => {
                    config.cooldown_after_timeout_seconds =
                        positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--deep-check-interval-accepts" => {
                    config.deep_check_interval_accepts =
                        positive_usize(&parser.value(&arg)?, &arg)?;
                }
                "--deep-check-interval-hours" => {
                    config.deep_check_interval_hours = positive_u64(&parser.value(&arg)?, &arg)?;
                }
                "--research-topic" => {
                    config.research_topic = non_empty_value(&parser.value(&arg)?, &arg)?;
                }
                "--research-slug" => {
                    config.research_slug = research_slug(&parser.value(&arg)?)?;
                }
                "--research-date" => {
                    config.research_date = research_date(&parser.value(&arg)?)?;
                }
                other if other.starts_with("--workspace=") => {
                    config.workspace = PathBuf::from(suffix(other, "--workspace="));
                }
                other if other.starts_with("--strategy=") => {
                    config.strategy = Strategy::parse(suffix(other, "--strategy="))?;
                }
                other if other.starts_with("--profile=") => {
                    config.profile = profile(suffix(other, "--profile=").to_owned())?;
                }
                other if other.starts_with("--model=") => {
                    config.model = Some(suffix(other, "--model=").to_owned());
                }
                other if other.starts_with("--codex-reasoning-effort=") => {
                    config.codex_reasoning_effort =
                        codex_reasoning_effort(suffix(other, "--codex-reasoning-effort="))?;
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
                other if other.starts_with("--categories=") => {
                    config.categories = Some(CategorySet::parse(suffix(other, "--categories="))?);
                }
                other if other.starts_with("--exclude-categories=") => {
                    excluded_categories =
                        Some(CategorySet::parse(suffix(other, "--exclude-categories="))?);
                }
                other if other.starts_with("--research-topic=") => {
                    config.research_topic =
                        non_empty_value(suffix(other, "--research-topic="), "--research-topic")?;
                }
                other if other.starts_with("--research-slug=") => {
                    config.research_slug = research_slug(suffix(other, "--research-slug="))?;
                }
                other if other.starts_with("--research-date=") => {
                    config.research_date = research_date(suffix(other, "--research-date="))?;
                }
                other if other.starts_with("--max-wall-clock-hours=") => {
                    config.max_wall_clock_hours = positive_u64(
                        suffix(other, "--max-wall-clock-hours="),
                        "--max-wall-clock-hours",
                    )?;
                }
                other if other.starts_with("--explore-timeout-seconds=") => {
                    config.explore_timeout_seconds = positive_u64(
                        suffix(other, "--explore-timeout-seconds="),
                        "--explore-timeout-seconds",
                    )?;
                }
                other if other.starts_with("--macro-explore-timeout-seconds=") => {
                    config.macro_explore_timeout_seconds = positive_u64(
                        suffix(other, "--macro-explore-timeout-seconds="),
                        "--macro-explore-timeout-seconds",
                    )?;
                }
                other => return Err(format!("unexpected argument: {other}")),
            }
        }
        apply_category_exclusions(&mut config, excluded_categories)?;
        Ok(config)
    }
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod parse_tests;

#[cfg(test)]
#[path = "unattended_tests.rs"]
mod unattended_tests;
