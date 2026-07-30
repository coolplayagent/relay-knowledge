use super::*;

#[test]
fn jobs_validate_and_resolve_bounded_parallelism() {
    assert_eq!(Jobs::parse("auto").expect("auto"), Jobs::Auto);
    assert_eq!(Jobs::parse("4").expect("fixed"), Jobs::Fixed(4));
    assert_eq!(Jobs::Auto.resolve(0), 1);
    assert_eq!(Jobs::Fixed(4).resolve(1), 4);
    assert!(Jobs::parse("0").is_err());
    assert!(Jobs::parse("many").is_err());
}
