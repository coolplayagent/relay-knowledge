pub fn array_field<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn usize_field(value: &Value, name: &str, default: usize) -> usize {
    value
        .get(name)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(default)
}

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
