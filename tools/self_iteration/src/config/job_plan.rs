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
        Self::from_inputs(config, cores, env_jobs)
    }

    fn from_inputs(config: &Config, cores: usize, env_jobs: Option<usize>) -> Self {
        let cores = cores.max(1);
        let global_default = env_jobs.unwrap_or(cores);
        Self {
            global: config.jobs.resolve(global_default),
            repositories: config.repo_jobs.resolve((cores / 2).max(1)),
            queries: config.query_jobs.resolve(cores),
        }
    }
}

#[cfg(test)]
#[path = "job_plan_tests.rs"]
mod job_plan_tests;
use super::model::Config;
