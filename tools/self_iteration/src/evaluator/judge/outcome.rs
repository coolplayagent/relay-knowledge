fn judge_outcome(text: &str, suite: &Value) -> (bool, bool, f64, String, Value) {
    let payload = extract_json_object(text)
        .and_then(|json| serde_json::from_str::<Value>(&json).ok())
        .unwrap_or_else(|| serde_json::json!({"passed": false, "overall_score": 0.0, "summary": "invalid judge JSON"}));
    let score = payload
        .get("overall_score")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let confidence = payload
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let anti_fixture = payload
        .get("scores")
        .and_then(|scores| scores.get("anti_fixture_special_casing"))
        .and_then(Value::as_f64)
        .unwrap_or(score);
    let contract_failures = judge_contract_failures(&payload, suite);
    let passed = payload
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && score
            >= suite
                .get("min_score")
                .and_then(Value::as_f64)
                .unwrap_or(0.75)
        && confidence
            >= suite
                .get("min_confidence")
                .and_then(Value::as_f64)
                .unwrap_or(0.6)
        && anti_fixture
            >= suite
                .get("min_anti_fixture_special_casing")
                .and_then(Value::as_f64)
                .unwrap_or(0.75)
        && contract_failures.is_empty();
    let mut message = payload
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("judge completed")
        .to_owned();
    if !contract_failures.is_empty() {
        message.push_str("; judge contract failures: ");
        message.push_str(&contract_failures.join(", "));
    }
    (passed, passed, score, message, payload)
}

fn judge_contract_failures(payload: &Value, suite: &Value) -> Vec<String> {
    let mut failures = judge_required_output_field_failures(payload);
    failures.extend(judge_dimension_failures(payload, suite));
    failures
}

fn judge_required_output_field_failures(payload: &Value) -> Vec<String> {
    required_judge_output_fields()
        .into_iter()
        .filter(|field| payload.get(*field).is_none_or(Value::is_null))
        .map(|field| format!("{field}=missing"))
        .collect()
}

fn judge_dimension_failures(payload: &Value, suite: &Value) -> Vec<String> {
    let min_dimension_score = suite
        .get("min_dimension_score")
        .and_then(Value::as_f64)
        .unwrap_or(0.65);
    let Some(scores) = payload.get("scores") else {
        return vec!["missing scores".to_owned()];
    };
    required_judge_dimensions(suite)
        .into_iter()
        .filter_map(|dimension| {
            let Some(value) = scores.get(&dimension).and_then(Value::as_f64) else {
                return Some(format!("{dimension}=missing"));
            };
            if !(0.0..=1.0).contains(&value) {
                return Some(format!("{dimension}={value:.3} outside 0.0..1.0"));
            }
            (value < min_dimension_score).then(|| {
                format!("{dimension}={value:.3} below min_dimension_score={min_dimension_score:.3}")
            })
        })
        .collect()
}

fn required_judge_output_fields() -> [&'static str; 10] {
    [
        "passed",
        "confidence",
        "overall_score",
        "scores",
        "summary",
        "evidence",
        "risks",
        "recommended_cases",
        "capability_delta",
        "research_gaps",
    ]
}

fn required_judge_dimensions(suite: &Value) -> Vec<String> {
    let configured = array_field(suite, "rubric_dimensions")
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if configured.is_empty() {
        [
            "research_alignment",
            "competitive_advantage",
            "architecture_soundness",
            "performance_generalization",
            "implementation_actionability",
            "anti_fixture_special_casing",
            "judge_evidence_quality",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
    } else {
        configured
    }
}

fn shell_split(value: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && (ch == '"' || ch == '\'') {
            quote = Some(ch);
        } else if quote.is_none() && ch.is_whitespace() {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if quote.is_some() {
        return Err("unterminated quote in command".to_owned());
    }
    if !current.is_empty() {
        parts.push(current);
    }
    Ok(parts)
}

fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end >= start).then(|| text[start..=end].to_owned())
}
