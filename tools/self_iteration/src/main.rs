mod cases;
mod codex;
mod command;
mod config;
mod evaluator;
mod git_ops;
mod history;
mod research_plan;
mod scoring;
mod unattended;
mod workflow;

use config::Config;

pub(crate) use workflow::{
    PersistInput, apply_candidate_documentation_gate, evaluate_candidate_for_patch,
    new_layer_run_id, number, persist_scored_run_with_score, print_score, unix_timestamp,
    write_adopted_optimization_document,
};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let exit_code = match Config::parse(args).and_then(workflow::run) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("[self-iterate] {error}");
            1
        }
    };
    std::process::exit(exit_code);
}
