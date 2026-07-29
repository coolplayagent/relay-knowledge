use super::*;

#[test]
fn category_rotation_starts_with_competitive() {
    let mut state = UnattendedState::new(100);

    assert_eq!(
        next_unattended_category(&mut state),
        EvaluationCategory::Competitive
    );
    assert_eq!(
        next_unattended_category(&mut state),
        EvaluationCategory::SemanticVector
    );
}
