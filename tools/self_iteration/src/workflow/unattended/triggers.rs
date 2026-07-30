use crate::{config::Config, history};

use super::super::number;
use super::{COMPETITIVE_GAP_EPSILON, UnattendedState};

pub(super) fn macro_trigger(
    config: &Config,
    paths: &history::HistoryPaths,
    state: &UnattendedState,
) -> Result<Option<String>, String> {
    if state.competitive_promotion_failures >= config.macro_after_competitive_failures {
        return Ok(Some(
            "competitive promotion failures reached threshold".to_owned(),
        ));
    }
    if state.consecutive_empty_candidates >= config.macro_after_empty_candidates {
        return Ok(Some("empty candidates reached macro threshold".to_owned()));
    }
    competitive_gap_trigger(paths)
}

fn competitive_gap_trigger(paths: &history::HistoryPaths) -> Result<Option<String>, String> {
    let latest = history::previous_scored_run_for_workload(paths, "fast", Some("competitive"))?;
    let best = history::best_accepted_run_for_workload(paths, "fast", Some("competitive"))?;
    let Some(latest) = latest else {
        return Ok(None);
    };
    let Some(best) = best else {
        return Ok(None);
    };
    let latest_value = number(&latest, "competitive_capability");
    let best_value = number(&best, "competitive_capability");
    if best_value - latest_value > COMPETITIVE_GAP_EPSILON {
        Ok(Some(format!(
            "competitive capability gap {:.6} exceeds {:.6}",
            best_value - latest_value,
            COMPETITIVE_GAP_EPSILON
        )))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
#[path = "triggers_tests.rs"]
mod triggers_tests;
