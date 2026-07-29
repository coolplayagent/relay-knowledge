use std::{collections::HashMap, ffi::OsString, path::PathBuf};

use super::{
    PlatformKind,
    platform::{TEMP, TMP, TMPDIR, normalize_key, platform_environment},
};

#[test]
fn windows_temp_prefers_temp_then_tmp_then_tmpdir() {
    let values = HashMap::from([
        (OsString::from(TMPDIR), OsString::from("/posix/tmp")),
        (OsString::from(TEMP), OsString::from("/windows/temp")),
        (OsString::from(TMP), OsString::from("/windows/tmp")),
    ]);

    let platform = platform_environment(&values, PlatformKind::Windows)
        .expect("platform environment should parse");

    assert_eq!(platform.temp_dir, Some(PathBuf::from("/windows/temp")));
}

#[test]
fn key_normalization_only_folds_windows_names() {
    assert_eq!(
        normalize_key(PlatformKind::Windows, OsString::from("mixed_Case")),
        OsString::from("MIXED_CASE")
    );
    assert_eq!(
        normalize_key(PlatformKind::Unix, OsString::from("mixed_Case")),
        OsString::from("mixed_Case")
    );
}
