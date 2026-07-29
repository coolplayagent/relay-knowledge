use super::*;

#[test]
fn score_math_uses_defaults_and_probability_bounds() {
    assert_eq!(average(&[], 0.25), 0.25);
    assert_eq!(average(&[0.2, 0.6], 0.0), 0.4);
    assert_eq!(clamp(-0.1), 0.0);
    assert_eq!(clamp(1.1), 1.0);
}
