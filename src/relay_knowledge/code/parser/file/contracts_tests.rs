//! File-parse contract tests.

use super::FileParseOutput;

#[test]
fn new_parse_output_starts_with_isolated_empty_buffers() {
    let output = FileParseOutput::new();

    assert!(output.symbols.is_empty());
    assert!(output.references.is_empty());
    assert!(output.reference_keys.is_empty());
}
