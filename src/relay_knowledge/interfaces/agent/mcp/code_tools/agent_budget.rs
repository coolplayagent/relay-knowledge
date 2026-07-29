use serde::Serialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct ExploreBudget {
    calls: usize,
    max_output_chars: usize,
    max_files: usize,
}

pub(super) fn explore_budget(file_count: usize) -> ExploreBudget {
    match file_count {
        0..=499 => ExploreBudget {
            calls: 1,
            max_output_chars: 15_000,
            max_files: 5,
        },
        500..=4_999 => ExploreBudget {
            calls: 2,
            max_output_chars: 30_000,
            max_files: 10,
        },
        5_000..=14_999 => ExploreBudget {
            calls: 3,
            max_output_chars: 45_000,
            max_files: 15,
        },
        _ => ExploreBudget {
            calls: 5,
            max_output_chars: 75_000,
            max_files: 25,
        },
    }
}

pub(super) fn apply_agent_code_budget(
    structured: &mut Value,
    budget: ExploreBudget,
    include_code: bool,
) {
    let mut outlined_count = 0usize;
    let mut truncated = structured["truncated"].as_bool().unwrap_or(false);
    if let Some(results) = structured.get_mut("results").and_then(Value::as_array_mut) {
        if results.len() > budget.max_files {
            results.truncate(budget.max_files);
            truncated = true;
        }
        if include_code {
            for result in results {
                if let Some(hit) = code_hit_object_mut(result) {
                    if outline_container_hit(hit) {
                        outlined_count += 1;
                    }
                }
            }
        }
    }

    structured["explore_budget"] = json!(budget);
    structured["truncated"] = Value::Bool(truncated);
    structured["agent_output"] = json!({
        "truncated": truncated,
        "outlined_container_count": outlined_count,
    });

    if enforce_serialized_budget(structured, budget.max_output_chars) {
        structured["truncated"] = Value::Bool(true);
        structured["agent_output"]["truncated"] = Value::Bool(true);
    }
}

fn enforce_serialized_budget(structured: &mut Value, max_output_chars: usize) -> bool {
    if serialized_len(structured) <= max_output_chars {
        return false;
    }

    let mut truncated = truncate_excerpts_to_budget(structured, 512);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= truncate_excerpts_to_budget(structured, 128);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= truncate_status_members(structured, 3);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= truncate_status_members(structured, 0);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= compact_repository_set_metadata(structured);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= compact_echoed_request(structured);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= compact_scope_filters(structured);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= compact_freshness_echoes(structured);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= compact_metadata(structured);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= compact_result_member_filters(structured);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= trim_results_to_budget(structured, max_output_chars);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= truncate_excerpts_to_budget(structured, 0);
    if serialized_len(structured) <= max_output_chars {
        return true;
    }
    truncated |= slim_echoed_request_for_audit(structured);

    truncated || serialized_len(structured) > max_output_chars
}

fn trim_results_to_budget(structured: &mut Value, max_output_chars: usize) -> bool {
    let mut trimmed = false;
    while serialized_len(structured) > max_output_chars {
        let Some(results) = structured.get_mut("results").and_then(Value::as_array_mut) else {
            return trimmed;
        };
        if results.is_empty() {
            return trimmed;
        }
        results.pop();
        trimmed = true;
    }
    trimmed
}

fn truncate_status_members(structured: &mut Value, keep: usize) -> bool {
    let Some(status) = structured.get_mut("status").and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(members) = status.get_mut("members").and_then(Value::as_array_mut) else {
        return false;
    };
    if members.len() <= keep {
        return false;
    }
    let omitted = members.len() - keep;
    members.truncate(keep);
    status.insert("members_omitted_by_agent_budget".to_owned(), json!(omitted));
    true
}

fn compact_repository_set_metadata(structured: &mut Value) -> bool {
    let Some(repository_set) = structured
        .get_mut("status")
        .and_then(|status| status.get_mut("repository_set"))
        .and_then(Value::as_object_mut)
    else {
        return false;
    };

    let mut compacted = false;
    for key in ["description", "default_ref_policy_json"] {
        if let Some(value) = repository_set.remove(key) {
            let chars = value.as_str().map_or(1, |text| text.chars().count());
            repository_set.insert(format!("{key}_omitted_by_agent_budget_chars"), json!(chars));
            compacted = true;
        }
    }
    compacted
}

fn compact_echoed_request(structured: &mut Value) -> bool {
    let Some(request) = structured.get_mut("request").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut compacted = compact_array_fields(request, ["path_filters", "language_filters"]);
    if let Some(repository) = request.get_mut("repository").and_then(Value::as_object_mut) {
        compacted |= compact_array_fields(repository, ["path_filters", "language_filters"]);
    }
    let compact_query = request
        .get("query")
        .and_then(Value::as_str)
        .filter(|query| query.chars().count() > 512)
        .map(|query| {
            let mut compact = query.chars().take(512).collect::<String>();
            compact.push_str("\n[truncated by MCP adaptive output budget]");
            compact
        });
    if let Some(query) = compact_query {
        request.insert("query".to_owned(), Value::String(query));
        compacted = true;
    }
    compacted
}

fn compact_scope_filters(structured: &mut Value) -> bool {
    let Some(scope) = structured.get_mut("scope").and_then(Value::as_object_mut) else {
        return false;
    };
    compact_array_fields(scope, ["path_filters", "language_filters"])
}

fn compact_freshness_echoes(structured: &mut Value) -> bool {
    let Some(freshness) = structured
        .get_mut("freshness")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    compact_array_fields(
        freshness,
        ["direct_source_read_paths", "agent_instructions"],
    )
}

fn compact_metadata(structured: &mut Value) -> bool {
    let Some(metadata) = structured
        .get_mut("metadata")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    compact_string_fields(metadata, ["request_id", "trace_id"], 128)
}

fn compact_result_member_filters(structured: &mut Value) -> bool {
    let Some(results) = structured.get_mut("results").and_then(Value::as_array_mut) else {
        return false;
    };

    let mut compacted = false;
    for result in results {
        let Some(member) = result.get_mut("member").and_then(Value::as_object_mut) else {
            continue;
        };
        compacted |= compact_array_fields(member, ["path_filters", "language_filters"]);
    }
    compacted
}

fn compact_array_fields<const N: usize>(object: &mut Map<String, Value>, keys: [&str; N]) -> bool {
    let mut compacted = false;
    for key in keys {
        if let Some(value) = object.remove(key) {
            let count = value.as_array().map_or(1, Vec::len);
            object.insert(format!("{key}_omitted_by_agent_budget"), json!(count));
            compacted = true;
        }
    }
    compacted
}

fn compact_string_fields<const N: usize>(
    object: &mut Map<String, Value>,
    keys: [&str; N],
    max_chars: usize,
) -> bool {
    let mut compacted = false;
    for key in keys {
        let Some(text) = object.get(key).and_then(Value::as_str) else {
            continue;
        };
        let chars = text.chars().count();
        if chars <= max_chars {
            continue;
        }
        let mut compact = text.chars().take(max_chars).collect::<String>();
        compact.push_str("\n[truncated by MCP adaptive output budget]");
        object.insert(key.to_owned(), Value::String(compact));
        object.insert(format!("{key}_omitted_by_agent_budget_chars"), json!(chars));
        compacted = true;
    }
    compacted
}

fn slim_echoed_request_for_audit(structured: &mut Value) -> bool {
    let Some(object) = structured.as_object_mut() else {
        return false;
    };
    let Some(request_value) = object.get("request").cloned() else {
        return false;
    };
    let Some(request) = request_value.as_object() else {
        object.remove("request");
        object.insert(
            "request_omitted_by_agent_budget".to_owned(),
            Value::Bool(true),
        );
        return true;
    };

    let mut slim = Map::new();
    if let Some(repository) = request_repository_scope(request) {
        slim.insert("repository".to_owned(), repository);
    }
    for key in ["set_alias", "freshness_policy", "limit"] {
        if let Some(value) = request.get(key) {
            slim.insert(key.to_owned(), value.clone());
        }
    }

    if slim.is_empty() {
        object.remove("request");
        object.insert(
            "request_omitted_by_agent_budget".to_owned(),
            Value::Bool(true),
        );
        return true;
    }

    let unchanged = slim
        .iter()
        .all(|(key, value)| request.get(key) == Some(value))
        && slim.len() == request.len();
    if unchanged {
        return false;
    }
    let omitted = request.len() - slim.len();
    slim.insert("fields_omitted_by_agent_budget".to_owned(), json!(omitted));
    object.insert("request".to_owned(), Value::Object(slim));
    true
}

fn request_repository_scope(request: &Map<String, Value>) -> Option<Value> {
    let repository = request.get("repository")?.as_object()?;
    let repository_name = repository.get("repository")?.as_str()?;
    Some(json!({ "repository": repository_name }))
}

fn code_hit_object_mut(result: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    if result.get("hit").is_some() {
        return result.get_mut("hit").and_then(Value::as_object_mut);
    }

    result.as_object_mut()
}

fn outline_container_hit(hit: &mut serde_json::Map<String, Value>) -> bool {
    let Some(excerpt) = hit.get("excerpt").and_then(Value::as_str) else {
        return false;
    };
    if !container_excerpt(excerpt) {
        return false;
    }
    let start_line = hit
        .get("line_range")
        .and_then(|range| range.get("start"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let outline = container_outline(excerpt, start_line);
    hit.insert("excerpt".to_owned(), Value::String(outline));
    hit.insert("source_outline".to_owned(), Value::Bool(true));

    true
}

fn container_excerpt(excerpt: &str) -> bool {
    let first = excerpt
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim_start)
        .unwrap_or_default();
    let normalized = first
        .strip_prefix("pub ")
        .or_else(|| first.strip_prefix("export "))
        .unwrap_or(first);

    ["class ", "struct ", "interface ", "enum ", "trait "]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

fn container_outline(excerpt: &str, start_line: u64) -> String {
    let mut lines = Vec::new();
    for (index, line) in excerpt.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "{" || trimmed == "}" || trimmed == "};" {
            continue;
        }
        if index == 0 || looks_like_member_signature(trimmed) {
            lines.push(format!("{}: {trimmed}", start_line + index as u64));
        }
        if lines.len() >= 32 {
            lines.push("[outline truncated]".to_owned());
            break;
        }
    }

    lines.join("\n")
}

fn looks_like_member_signature(line: &str) -> bool {
    let normalized = line
        .strip_prefix("pub ")
        .or_else(|| line.strip_prefix("public "))
        .or_else(|| line.strip_prefix("private "))
        .or_else(|| line.strip_prefix("protected "))
        .or_else(|| line.strip_prefix("static "))
        .or_else(|| line.strip_prefix("virtual "))
        .unwrap_or(line);

    normalized.starts_with("fn ")
        || normalized.starts_with("def ")
        || normalized.starts_with("function ")
        || normalized.starts_with("async ")
        || normalized.contains('(') && (normalized.ends_with(';') || normalized.ends_with('{'))
}

fn truncate_excerpts_to_budget(structured: &mut Value, max_excerpt_chars: usize) -> bool {
    let Some(results) = structured.get_mut("results").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut truncated = false;
    for result in results {
        let Some(hit) = code_hit_object_mut(result) else {
            continue;
        };
        let Some(excerpt) = hit.get("excerpt").and_then(Value::as_str) else {
            continue;
        };
        if excerpt.chars().count() <= max_excerpt_chars {
            continue;
        }
        let mut compact = excerpt.chars().take(max_excerpt_chars).collect::<String>();
        compact.push_str("\n[truncated by MCP adaptive output budget]");
        hit.insert("excerpt".to_owned(), Value::String(compact));
        truncated = true;
    }
    truncated
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_string(value)
        .map(|text| text.len())
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
#[path = "agent_budget_tests.rs"]
mod tests;
