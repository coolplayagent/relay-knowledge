use crate::{
    config::{Config, Mode},
    history, research_plan,
};

use super::{loop_control::run_loop, manual_evaluation::run_evaluate};

pub(crate) fn run(mut config: Config) -> Result<i32, String> {
    config.workspace = config
        .workspace
        .canonicalize()
        .map_err(|error| format!("invalid workspace {}: {error}", config.workspace.display()))?;
    if matches!(config.mode, Mode::ResearchPlan) {
        println!(
            "{}",
            research_plan::render(research_plan::ResearchPlanInput {
                topic: &config.research_topic,
                slug: &config.research_slug,
                date: &config.research_date,
            })
        );
        return Ok(0);
    }
    let paths = history::HistoryPaths::new(&config.workspace);
    paths.ensure()?;
    match config.mode {
        Mode::Chart => {
            let (csv, svg) = history::export_history(&paths)?;
            println!("score csv: {}", csv.display());
            println!("score svg: {}", svg.display());
            Ok(0)
        }
        Mode::Evaluate => run_evaluate(&config, &paths),
        Mode::Once => {
            config.max_iterations = Some(1);
            run_loop(&config, &paths)
        }
        Mode::Loop => run_loop(&config, &paths),
        Mode::ResearchPlan => unreachable!("research plan returns before history initialization"),
    }
}
