use std::{collections::BTreeSet, path::PathBuf};

#[test]
fn readmes_document_supported_long_options() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut source = String::new();
    for entry in std::fs::read_dir(manifest.join("src/config")).expect("config source directory") {
        let path = entry.expect("config source entry").path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            source.push_str(
                &std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
            );
        }
    }
    let readme = std::fs::read_to_string(manifest.join("README.md")).expect("README.md");
    let readme_zh =
        std::fs::read_to_string(manifest.join("README.zh-CN.md")).expect("README.zh-CN.md");
    let options = long_options_from_source(&source);

    for option in options {
        assert!(
            readme.contains(&option),
            "README.md should document {option}"
        );
        assert!(
            readme_zh.contains(&option),
            "README.zh-CN.md should document {option}"
        );
    }
}

fn long_options_from_source(source: &str) -> BTreeSet<String> {
    source
        .split('"')
        .filter_map(|item| item.strip_prefix("--"))
        .map(|item| {
            let name = item
                .split(|ch: char| ch == '=' || ch.is_whitespace())
                .next()
                .unwrap_or_default();
            format!("--{name}")
        })
        .filter(|option| option.len() > 2 && !option.contains('{'))
        .collect()
}
