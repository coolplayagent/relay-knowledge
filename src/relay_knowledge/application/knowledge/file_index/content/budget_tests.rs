use super::*;

#[test]
fn content_scan_budget_accounts_attempted_reads() {
    let mut content_scan_bytes = 0;

    assert!(!reserve_content_read_with_budget(
        &mut content_scan_bytes,
        4,
        6
    ));
    assert!(reserve_content_read_with_budget(
        &mut content_scan_bytes,
        4,
        6
    ));
    assert_eq!(content_scan_bytes, 6);
}
