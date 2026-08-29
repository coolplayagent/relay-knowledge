//! Narrow wall-clock contract and system implementation for persisted timestamps.

use std::{error::Error, fmt, time::SystemTime};

pub(crate) trait Clock {
    fn now_millis(&self) -> Result<u64, ClockError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> Result<u64, ClockError> {
        unix_millis(SystemTime::now())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClockError {
    BeforeUnixEpoch,
    MillisecondsOverflow,
}

impl fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BeforeUnixEpoch => formatter.write_str("system clock is before Unix epoch"),
            Self::MillisecondsOverflow => {
                formatter.write_str("system clock milliseconds exceed u64 range")
            }
        }
    }
}

impl Error for ClockError {}

pub(crate) fn system_now_millis() -> Result<u64, ClockError> {
    SystemClock.now_millis()
}

pub(crate) fn system_now_millis_or_zero() -> u64 {
    system_now_millis().unwrap_or(0)
}

fn unix_millis(now: SystemTime) -> Result<u64, ClockError> {
    let elapsed = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| ClockError::BeforeUnixEpoch)?;
    u64::try_from(elapsed.as_millis()).map_err(|_| ClockError::MillisecondsOverflow)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
