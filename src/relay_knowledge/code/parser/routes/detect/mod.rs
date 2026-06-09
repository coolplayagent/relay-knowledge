pub(in crate::code::parser) mod express;
pub(in crate::code::parser) mod flask;
pub(in crate::code::parser) mod javascript;
pub(in crate::code::parser) mod shared;
pub(in crate::code::parser) mod spring;

pub(in crate::code::parser) struct RouteCandidate {
    pub(in crate::code::parser) url: String,
    pub(in crate::code::parser) http_method: String,
    pub(in crate::code::parser) handler_name: String,
    pub(in crate::code::parser) framework: String,
    pub(in crate::code::parser) line: usize,
}

pub(in crate::code::parser) const ANONYMOUS_ROUTE_HANDLER_NAME: &str = "anonymous";

pub(in crate::code::parser) fn detect_routes(
    language_id: &str,
    content: &str,
) -> Vec<RouteCandidate> {
    match language_id {
        "javascript" | "jsx" | "typescript" | "tsx" => express::detect_express_routes(content),
        "python" => flask::detect_flask_routes(content),
        "java" => spring::detect_spring_routes(content),
        _ => Vec::new(),
    }
}
