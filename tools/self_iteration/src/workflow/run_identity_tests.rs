use super::*;

#[test]
fn manual_evaluate_run_id_uses_unique_patch_namespace() {
    let run_id = new_manual_evaluate_run_id();

    assert!(run_id.starts_with("manual-evaluate-"));
    assert!(run_id.len() > "manual-evaluate-".len());
}

#[test]
fn generated_run_ids_are_process_scoped_and_distinct() {
    let first = new_run_id();
    let second = new_run_id();

    assert!(first.starts_with("run-"));
    assert!(first.ends_with(&format!("-{}", std::process::id())));
    assert_ne!(first, second);
}
