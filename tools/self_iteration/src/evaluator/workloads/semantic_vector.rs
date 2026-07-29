fn semantic_vector_runtime_profile(env: &BTreeMap<String, String>) -> Value {
    let semantic_backend = normalized_env(env, "RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "local");
    let vector_backend = normalized_env(env, "RELAY_KNOWLEDGE_VECTOR_BACKEND", "local");
    let external_requested = semantic_backend == "external" || vector_backend == "external";
    let required = [
        "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
        "RELAY_KNOWLEDGE_EMBEDDING_API_KEY",
        "RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL",
        "RELAY_KNOWLEDGE_EMBEDDING_DIMENSION",
    ];
    let missing = required
        .iter()
        .filter(|name| {
            external_requested
                && env
                    .get(**name)
                    .map(|value| value.trim().is_empty())
                    .unwrap_or(true)
        })
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    serde_json::json!({
        "semantic_backend": semantic_backend,
        "vector_backend": vector_backend,
        "external_requested": external_requested,
        "missing_external_env": missing,
    })
}

fn semantic_vector_env_check(profile: &Value) -> CommandResult {
    let missing = profile
        .get("missing_external_env")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let passed = missing.is_empty();
    CommandResult {
        name: "semantic_vector_external_env".to_owned(),
        command: vec!["validate".to_owned(), "semantic-vector-env".to_owned()],
        exit_code: if passed { 0 } else { 1 },
        duration_ms: 0,
        stdout: profile.to_string(),
        stderr: if passed {
            String::new()
        } else {
            format!("missing external semantic/vector env: {missing:?}")
        },
    }
}

fn validate_provider_probe(result: &mut CommandResult) -> bool {
    if !result.passed() {
        return false;
    }
    let Some(payload) = parse_json_output_value(&result.stdout) else {
        result.exit_code = 1;
        result.stderr = "provider probe returned invalid JSON".to_owned();
        return false;
    };
    if payload.get("ok").and_then(Value::as_bool).unwrap_or(true) {
        return true;
    }
    result.exit_code = 1;
    result.stderr = payload
        .get("error")
        .or_else(|| payload.get("error_code"))
        .and_then(Value::as_str)
        .unwrap_or("provider probe reported ok=false")
        .to_owned();
    false
}

fn semantic_vector_ingest_command(binary: &Path, scope: &str, evidence: &Value) -> Vec<String> {
    let mut command = vec![
        binary.display().to_string(),
        "ingest".to_owned(),
        "--source".to_owned(),
        scope.to_owned(),
        "--content".to_owned(),
        string_or(evidence, "content", "").to_owned(),
    ];
    for entity in string_vec(evidence, "entities") {
        command.extend(["--entity".to_owned(), entity]);
    }
    command.extend(["--format".to_owned(), "json".to_owned()]);
    command
}

fn semantic_vector_query_command(binary: &Path, scope: &str, case: &Value) -> Vec<String> {
    vec![
        binary.display().to_string(),
        "query".to_owned(),
        string_or(case, "query", "").to_owned(),
        "--source".to_owned(),
        scope.to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        number_or(case, "limit", 10).to_string(),
        "--format".to_owned(),
        "json".to_owned(),
    ]
}

fn score_semantic_vector_case(case: &Value, result: &CommandResult) -> CaseObservation {
    if !result.passed() {
        return failed_case(case, "semantic_vector", "semantic_vector", result);
    }
    let payload = match parse_json_case_output(case, "semantic_vector", "semantic_vector", result)
    {
        Ok(payload) => payload,
        Err(observation) => return *observation,
    };
    let hits = score_array_field(&payload, "results");
    let expected = score_array_field(case, "expected");
    let forbidden = score_array_field(case, "forbidden");
    let max_rank = number_or(case, "max_rank", 1) as usize;
    let rank = hits
        .iter()
        .enumerate()
        .find_map(|(index, hit)| hit_matches_any(hit, expected).then_some(index + 1));
    let false_positives = hits
        .iter()
        .filter(|hit| hit_matches_any(hit, forbidden))
        .count();
    let missing_sources =
        missing_required_sources(case, rank.and_then(|index| hits.get(index - 1)), hits);
    let missing_backends = missing_required_backends(case, &payload);
    let mut passed = (expected.is_empty() || rank.is_some_and(|rank| rank <= max_rank))
        && false_positives == 0
        && missing_sources.is_empty()
        && missing_backends.is_empty();
    let mut final_rank = rank;
    if case
        .get("expect_empty")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        passed = hits.is_empty();
        final_rank = passed.then_some(0);
    }
    CaseObservation {
        case_id: string_or(case, "id", "case").to_owned(),
        repository: "semantic_vector".to_owned(),
        passed,
        guardrail: is_guardrail_case(case),
        rank: final_rank,
        max_rank,
        false_positive_count: false_positives,
        message: format!(
            "results={} rank={final_rank:?} missing_sources={missing_sources:?} missing_backends={missing_backends:?}",
            hits.len()
        ),
        objective: "semantic_vector".to_owned(),
        score_override: None,
    }
}

fn missing_required_sources(
    case: &Value,
    matched_hit: Option<&Value>,
    hits: &[Value],
) -> Vec<String> {
    let required = string_vec(case, "required_sources");
    if required.is_empty() {
        return Vec::new();
    }
    let observed = if let Some(hit) = matched_hit {
        hit_sources(hit)
    } else {
        hits.iter().flat_map(hit_sources).collect::<Vec<_>>()
    };
    required
        .into_iter()
        .filter(|source| !observed.contains(source))
        .collect()
}

fn hit_sources(hit: &Value) -> Vec<String> {
    hit.get("retriever_sources")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn missing_required_backends(case: &Value, payload: &Value) -> Vec<String> {
    let required = case
        .get("required_backend_states")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let states = payload
        .get("backend_statuses")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|status| {
                    Some((
                        status.get("source")?.as_str()?.to_owned(),
                        status.get("state")?.as_str()?.to_owned(),
                    ))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    required
        .into_iter()
        .filter_map(|(source, allowed)| {
            let allowed = allowed
                .as_array()
                .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            let current = states.get(&source).map(String::as_str);
            (!current.is_some_and(|state| allowed.contains(&state)))
                .then(|| format!("{}:{}", source, current.unwrap_or("missing")))
        })
        .collect()
}
