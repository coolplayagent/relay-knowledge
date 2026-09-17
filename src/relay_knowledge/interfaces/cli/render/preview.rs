//! Human-readable preview details with explicit list truncation and escaped diagnostics.

use serde_json::Value;

pub(super) fn render_scope_preview(preview: &Value) -> String {
    let mut output = format!(
        "preview files={} bytes={} unsupported={}",
        preview["selected_file_count"].as_u64().unwrap_or(0),
        preview["selected_byte_count"].as_u64().unwrap_or(0),
        preview["unsupported_file_count"].as_u64().unwrap_or(0),
    );
    for key in ["expected_degraded_files", "excluded_paths"] {
        let count = preview[key].as_array().map_or(0, Vec::len);
        let truncated = preview[format!("{key}_truncated")]
            .as_bool()
            .unwrap_or(false);
        output.push_str(&format!(" {key}={count} {key}_truncated={truncated}"));
    }
    for key in ["expected_degraded_files", "excluded_paths"] {
        if let Some(files) = preview[key].as_array() {
            for file in files {
                // JSON quoting keeps embedded newlines and control characters on one line.
                output.push_str(&format!(
                    "\n{key} path={} reason={}",
                    file["path"], file["reason"]
                ));
            }
        }
        if preview[format!("{key}_truncated")].as_bool() == Some(true) {
            output.push_str(&format!(
                "\n{key}: showing the first 50 entries; more files were observed."
            ));
            if key == "expected_degraded_files" {
                output.push_str(" Remaining parser batches may not have been checked.");
            }
        }
    }
    output
}
