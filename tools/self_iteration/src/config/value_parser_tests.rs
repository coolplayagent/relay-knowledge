use super::*;

#[test]
fn scalar_parsers_reject_zero_and_invalid_metadata() {
    assert_eq!(positive_u64("3", "--max-wall-clock-hours").expect("u64"), 3);
    assert_eq!(positive_usize("2", "--jobs").expect("usize"), 2);
    assert!(positive_u64("0", "--max-wall-clock-hours").is_err());
    assert!(positive_usize("invalid", "--jobs").is_err());
    assert!(research_slug("Graph DB").is_err());
    assert!(research_date("20260730").is_err());
}

#[test]
fn parser_consumes_mode_and_reports_missing_option_values() {
    let mut parser = Parser::new(vec!["once".to_owned(), "--profile".to_owned()]);

    assert_eq!(parser.take_mode(), Some(Mode::Once));
    assert_eq!(parser.next().as_deref(), Some("--profile"));
    assert!(parser.value("--profile").is_err());
}
