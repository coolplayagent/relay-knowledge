mod candidate_git;
mod cases;
mod codex;
mod command;
mod config;
mod evaluator;
mod history;
mod research_plan;
mod scoring;
mod workflow;

use config::Config;

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
