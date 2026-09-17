use crate::code::feature_flags::FeatureFlagFileInput;

#[test]
fn configuration_scan_work_suite_cached_lines_preserve_prefix_boundaries_and_reuse_storage() {
    for content in ["", "a", "\n", "\r", "\r\n", "α\r\nβ\rc\n\nd\r\r\n"] {
        let input = FeatureFlagFileInput {
            line_index: Default::default(),
            syntax_root: None,
            repository_id: "repo",
            source_scope: "scope",
            file_id: "file",
            path: "source.py",
            language_id: "python",
            content,
            config_facts: &[],
        };
        assert!(input.line_index.0.get().is_none());
        let mut expected = 1;
        for offset in 0..=content.len() {
            if offset > 0 {
                let bytes = content.as_bytes();
                if bytes[offset - 1] == b'\r'
                    || (bytes[offset - 1] == b'\n' && (offset == 1 || bytes[offset - 2] != b'\r'))
                {
                    expected += 1;
                }
            }
            assert_eq!(
                input.line_number(offset),
                expected,
                "{content:?} at {offset}"
            );
        }
        let allocated = input.line_index.0.get().unwrap().as_ptr();
        for offset in (0..=content.len()).rev() {
            input.line_number(offset);
        }
        assert_eq!(allocated, input.line_index.0.get().unwrap().as_ptr());
    }
}
