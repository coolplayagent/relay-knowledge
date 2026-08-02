use super::*;

#[test]
fn rerank_mode_parses_and_renders_stable_configuration_values() {
    for (mode, label) in [
        (RerankMode::Local, "local"),
        (RerankMode::External, "external"),
        (RerankMode::Disabled, "disabled"),
    ] {
        assert_eq!(mode.as_str(), label);
        assert_eq!(
            RerankMode::parse(&label.to_ascii_uppercase()).expect("mode should parse"),
            mode
        );
    }

    let error = RerankMode::parse("remote").expect_err("unknown mode should fail");
    assert_eq!(
        error.to_string(),
        "rerank backend 'remote' must be local, external, or disabled"
    );
}
