fn average(values: &[f64], default: f64) -> f64 {
    if values.is_empty() {
        default
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn clamp(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}
