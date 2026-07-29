use crate::config::{Config, EvaluationCategory};

use super::{CATEGORY_ROTATION, UnattendedState};

pub(super) fn update_unattended_rejection_counters(
    state: &mut UnattendedState,
    category: EvaluationCategory,
) {
    state.consecutive_promotion_failures += 1;
    if category == EvaluationCategory::Competitive {
        state.competitive_promotion_failures += 1;
    }
}

pub(super) fn next_unattended_category(state: &mut UnattendedState) -> EvaluationCategory {
    let category = CATEGORY_ROTATION[state.category_index % CATEGORY_ROTATION.len()];
    state.category_index += 1;
    category
}

pub(super) fn selected_or_default_category(config: &Config) -> EvaluationCategory {
    let Some(categories) = &config.categories else {
        return EvaluationCategory::Competitive;
    };
    for category in CATEGORY_ROTATION {
        if categories.contains(category) {
            return category;
        }
    }
    EvaluationCategory::Competitive
}

#[cfg(test)]
#[path = "category_rotation_tests.rs"]
mod category_rotation_tests;
