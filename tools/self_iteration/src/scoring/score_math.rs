pub(super) fn average(values: &[f64], default: f64) -> f64 {
    if values.is_empty() {
        default
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

pub(super) fn clamp(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
#[path = "score_math_tests.rs"]
mod score_math_tests;
