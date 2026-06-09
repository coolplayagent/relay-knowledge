//! Web framework route detection for parser-owned, in-memory source analysis.

mod detect;

pub(in crate::code::parser) use detect::{ANONYMOUS_ROUTE_HANDLER_NAME, detect_routes};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "review_tests.rs"]
mod review_tests;
