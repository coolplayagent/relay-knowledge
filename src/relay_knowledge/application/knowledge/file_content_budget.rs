pub(super) fn reserve_content_read_with_budget(
    content_scan_bytes: &mut usize,
    read_bytes: u64,
    max_content_scan_bytes: usize,
) -> bool {
    let Ok(read_bytes) = usize::try_from(read_bytes) else {
        *content_scan_bytes = max_content_scan_bytes;
        return true;
    };
    if content_scan_bytes.saturating_add(read_bytes) > max_content_scan_bytes {
        *content_scan_bytes = max_content_scan_bytes;
        true
    } else {
        *content_scan_bytes = content_scan_bytes.saturating_add(read_bytes);
        false
    }
}
