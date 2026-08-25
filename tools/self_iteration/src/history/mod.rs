use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct HistoryPaths {
    pub root: PathBuf,
    pub reports: PathBuf,
    pub patches: PathBuf,
    pub work: PathBuf,
    pub memory: PathBuf,
    pub memory_index: PathBuf,
    pub memory_summaries: PathBuf,
    pub memory_details: PathBuf,
    pub memory_artifacts: PathBuf,
    pub unattended_state: PathBuf,
    pub runs_jsonl: PathBuf,
    pub score_csv: PathBuf,
    pub score_svg: PathBuf,
}

impl HistoryPaths {
    pub fn new(workspace: &Path) -> Self {
        let root = workspace
            .join(".git")
            .join("relay-knowledge-self-iteration");
        Self {
            reports: root.join("reports-v2"),
            patches: root.join("patches-v2"),
            work: root.join("work-v2"),
            memory: root.join("memory"),
            memory_index: root.join("memory").join("index.jsonl"),
            memory_summaries: root.join("memory").join("summaries"),
            memory_details: root.join("memory").join("details"),
            memory_artifacts: root.join("memory").join("artifacts"),
            unattended_state: root.join("unattended-state-v2.json"),
            runs_jsonl: root.join("runs-v2.jsonl"),
            score_csv: root.join("score-v2.csv"),
            score_svg: root.join("score-v2.svg"),
            root,
        }
    }

    pub fn ensure(&self) -> Result<(), String> {
        for path in [
            &self.root,
            &self.reports,
            &self.patches,
            &self.work,
            &self.memory,
            &self.memory_summaries,
            &self.memory_details,
            &self.memory_artifacts,
        ] {
            fs::create_dir_all(path)
                .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
        }
        Ok(())
    }
}

pub(crate) mod memory;
pub(crate) mod synthesis;

mod export;
mod persistence;
mod run_state;
mod runs;

pub use export::export_history;
pub use persistence::{RunRecordInput, append_run, make_run_record, write_report};
pub use run_state::{adopted, is_evaluate_run};
pub use runs::{
    best_accepted_run_for_profile, best_accepted_run_for_profile_across_product_binaries,
    best_accepted_run_for_workload, load_runs, previous_scored_run,
    previous_scored_run_for_workload,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
