//! Fail-closed public evidence projection; private inputs are never interpolated.

use sha2::{Digest, Sha256};

use super::{FeedbackError, FeedbackPayload, FeedbackReport};

/// SHA-256 binding for local raw inputs or public payloads, kept as separate scopes.
pub fn feedback_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Projects only caller-designated narrative fields, rejecting recognizable secrets.
///
/// Arbitrary prose cannot be proven public by a secret scanner. Raw evidence and
/// diagnostics are therefore always excluded, even if they contain no token pattern.
pub fn prepare_payload(
    report: &FeedbackReport,
    cli_version: &str,
    platform: &str,
    marker: &str,
) -> Result<FeedbackPayload, FeedbackError> {
    report.validate()?;
    validate_marker(marker)?;
    for metadata in [cli_version, platform] {
        if metadata.is_empty()
            || metadata.len() > 128
            || !metadata
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+".contains(&byte))
        {
            return Err(FeedbackError("invalid public runtime metadata".into()));
        }
    }
    let mut fields = vec![
        ("Intent".to_owned(), report.intent.clone()),
        ("Expected".to_owned(), report.expected.clone()),
        ("Actual".to_owned(), report.actual.clone()),
        ("Impact".to_owned(), report.impact.clone()),
    ];
    for observation in &report.observations {
        let origin = serde_json::to_string(&observation.origin)
            .map_err(|error| FeedbackError(error.to_string()))?;
        fields.push((
            format!("Observation ({origin}; caller supplied)"),
            observation.text.clone(),
        ));
    }
    if let Some(reproduction) = &report.reproduction {
        fields.push((
            "Minimal scenario (inert text)".into(),
            reproduction.scenario.clone(),
        ));
        fields.push(("Scenario criterion".into(), reproduction.expected.clone()));
    }
    for (name, text) in &fields {
        if sensitive(text) {
            return Err(FeedbackError(format!(
                "evidence-insufficient: {name} contains potentially private data; provide a public minimal summary"
            )));
        }
    }
    let kind =
        serde_json::to_value(report.kind).map_err(|error| FeedbackError(error.to_string()))?;
    let kind = kind.as_str().unwrap_or("feedback");
    let mut title = format!("[{kind}] ");
    for character in report.intent.chars() {
        let character = if matches!(character, '\r' | '\n') {
            ' '
        } else {
            character
        };
        if title.len() + character.len_utf8() > 256 {
            break;
        }
        title.push(character);
    }
    let mut body = format!(
        "Software experience feedback\n\nKind: {kind}\nCLI version: {cli_version}\nPlatform: {platform}\n\nObservations are reports, not independently established defects.\n"
    );
    for (name, text) in fields {
        body.push_str(&format!("\n## {name}\n\n"));
        for line in text.lines() {
            body.push_str("> ");
            body.push_str(&line.replace('<', "&lt;").replace('>', "&gt;"));
            body.push('\n');
        }
    }
    body.push_str(
        "\nRaw evidence, diagnostics, trace identifiers, and private content were omitted.\n\n",
    );
    let public_digest = feedback_digest(
        &serde_json::to_vec(&(&title, &body)).map_err(|error| FeedbackError(error.to_string()))?,
    );
    let dedup_marker = format!("<!-- relay-feedback:{public_digest} -->");
    body.push_str(&dedup_marker);
    body.push('\n');
    body.push_str(marker);
    body.push('\n');
    if body.len() > 65_536 {
        return Err(FeedbackError(
            "evidence-insufficient: public payload exceeds 65536 bytes; provide a smaller public summary".into(),
        ));
    }
    let digest = feedback_digest(
        &serde_json::to_vec(&(&title, &body)).map_err(|error| FeedbackError(error.to_string()))?,
    );
    let mut omitted_evidence = Vec::new();
    if !report.evidence.is_empty() {
        omitted_evidence.push("raw evidence".into());
    }
    if report.diagnostics.is_some() {
        omitted_evidence.push("diagnostics".into());
    }
    if report.trace_id.is_some() || report.request_id.is_some() {
        omitted_evidence.push("trace/request identifiers".into());
    }
    Ok(FeedbackPayload {
        title,
        body,
        dedup_marker,
        digest,
        omitted_evidence,
    })
}

fn validate_marker(marker: &str) -> Result<(), FeedbackError> {
    let nonce = marker
        .strip_prefix("<!-- relay-feedback:")
        .and_then(|value| value.strip_suffix(" -->"));
    if !nonce.is_some_and(|value| {
        matches!(value.len(), 32 | 64)
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(FeedbackError("invalid feedback correlation marker".into()));
    }
    Ok(())
}

fn sensitive(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if text.contains('@')
        || text
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return true;
    }
    if [
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "sk-",
        "akia",
        "xoxb-",
        "xoxp-",
        "bearer ",
        "basic ",
        "-----begin",
        "password",
        "passwd",
        "secret",
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
        "authorization:",
        "token=",
        "token:",
        "token\":",
        "client_secret",
        "cookie:",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return true;
    }
    let compact: String = lower
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '\'' | '"'))
        .collect();
    if compact.contains("token=") || compact.contains("token:") {
        return true;
    }
    if lower.contains("~/")
        || lower.contains("\\\\")
        || lower.contains("file://")
        || lower.contains("/home/")
        || lower.contains("/users/")
    {
        return true;
    }
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        let windows_drive = byte.is_ascii_alphabetic()
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric())
            && bytes.get(index + 1) == Some(&b':')
            && bytes
                .get(index + 2)
                .is_some_and(|next| matches!(next, b'/' | b'\\'));
        if windows_drive {
            return true;
        }
        if *byte != b'/' {
            return false;
        }
        let starts_value = index == 0
            || bytes[index - 1].is_ascii_whitespace()
            || b"=:'\"`([{,;".contains(&bytes[index - 1]);
        let public_url_prefix = index > 0
            && bytes[index - 1] == b':'
            && bytes.get(index + 1) == Some(&b'/')
            && (lower[..index - 1].ends_with("https") || lower[..index - 1].ends_with("http"));
        starts_value && !public_url_prefix
    })
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
