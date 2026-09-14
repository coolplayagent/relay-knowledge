use super::{
    clean_scalar, design_heading_kind, first_call_arg, json_string_pair, markdown_heading,
    terraform_block, toml_section, yaml_value,
};

#[test]
fn scalar_and_pair_parsers_normalize_manifest_values() {
    assert_eq!(clean_scalar(" ('relay'), "), "relay");
    assert_eq!(toml_section("[[bin]]"), Some("bin"));
    assert_eq!(
        json_string_pair(r#"  "build": "cargo build","#),
        Some(("build".to_owned(), "cargo build".to_owned()))
    );
    assert_eq!(
        yaml_value("image: \"relay:latest\"", "image"),
        Some("relay:latest".to_owned())
    );
}

#[test]
fn call_and_terraform_parsers_keep_the_first_declared_target() {
    assert_eq!(
        first_call_arg("add_executable(relay src/main.c)", "add_executable"),
        Some("relay".to_owned())
    );
    assert_eq!(
        terraform_block(r#"resource "aws_s3_bucket" "artifacts" {"#, "resource "),
        Some(("aws_s3_bucket".to_owned(), "artifacts".to_owned()))
    );
}

#[test]
fn markdown_headings_map_only_known_design_concepts() {
    assert_eq!(
        markdown_heading("## Runtime Architecture"),
        Some("Runtime Architecture".to_owned())
    );
    assert_eq!(
        design_heading_kind("Runtime Architecture", "docs/runtime.md"),
        Some("architecture")
    );
    assert_eq!(design_heading_kind("Getting Started", "README.md"), None);
    assert_eq!(design_heading_kind("Chapter Index", "README.md"), None);
    assert_eq!(
        design_heading_kind("Getting Started", "docs/guide.md"),
        None
    );
}
