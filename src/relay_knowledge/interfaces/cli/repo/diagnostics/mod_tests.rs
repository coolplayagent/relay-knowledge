use super::*;
#[test]
fn diagnostics_parser_validates_bounds_and_preserves_cursor() {
    let tokens = [
        "demo", "--ref", "HEAD", "--path", "src", "--limit", "2", "--cursor", "token",
    ]
    .map(str::to_owned);
    let RepoCommand::Diagnostics(request) = parse(&tokens).unwrap() else {
        panic!("wrong command")
    };
    assert_eq!(request.limit, 2);
    assert_eq!(request.cursor.as_deref(), Some("token"));
    assert!(parse(&["demo".into(), "--limit".into(), "201".into()]).is_err());
    assert!(parse(&["demo".into(), "--unknown".into()]).is_err());
    assert!(parse(&[]).is_err());
}
