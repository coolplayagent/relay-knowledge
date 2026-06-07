mod detect;

pub(in crate::code::parser) use detect::detect_routes;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
