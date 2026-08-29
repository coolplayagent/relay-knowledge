use std::time::{Duration, SystemTime};

use super::*;

#[test]
fn converts_epoch_relative_time_without_truncation() {
    assert_eq!(
        unix_millis(SystemTime::UNIX_EPOCH + Duration::from_micros(1_234_567)),
        Ok(1_234)
    );
}

#[test]
fn rejects_time_before_the_unix_epoch() {
    assert_eq!(
        unix_millis(SystemTime::UNIX_EPOCH - Duration::from_millis(1)),
        Err(ClockError::BeforeUnixEpoch)
    );
}
