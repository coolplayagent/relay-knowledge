//! Aggregates command metadata in stable CLI display order.

mod core;
mod map;
mod operations;
mod service;

use super::{CliCommandSpec, files, repo, repo_set};

pub(super) fn command_specs() -> Vec<CliCommandSpec> {
    let mut commands = core::knowledge_commands();
    commands.extend([
        files::files_index(),
        files::files_query(),
        files::files_content(),
        repo::repo_list(),
        repo::repo_register(),
        repo::repo_remove(),
        repo::repo_index(),
        repo::repo_index_worker(),
        repo::repo_scope_preview(),
        repo::repo_update(),
        repo::repo_query(),
        repo::repo_graph(),
        repo::repo_context(),
        repo::repo_feature_flags(),
        repo::repo_impact(),
        repo::repo_view(),
        repo::repo_status(),
        repo::repo_report(),
        repo::repo_software(),
        repo::repo_business(),
        repo_set::repo_set(),
    ]);
    commands.extend(map::command_specs());
    commands.extend(core::graph_commands());
    commands.extend(operations::command_specs());
    commands.extend(core::diagnostic_commands());
    commands.extend(service::command_specs());
    commands.extend(core::setup_and_meta_commands());
    commands
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
