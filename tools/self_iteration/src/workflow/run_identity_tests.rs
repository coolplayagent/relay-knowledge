use super::*;

#[test]
fn manual_evaluate_run_id_uses_unique_patch_namespace() {
    let run_id = new_manual_evaluate_run_id();

    assert!(run_id.starts_with("manual-evaluate-"));
    assert!(run_id.len() > "manual-evaluate-".len());
}
