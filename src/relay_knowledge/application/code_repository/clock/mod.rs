//! Code-repository wall-clock boundary backed by the shared system clock.

pub(super) use crate::clock::system_now_millis_or_zero as now_millis;
