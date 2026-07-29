mod affected_scope;
mod architecture;
mod builder;
mod business_domains;
mod dependency_tour;
mod process_flow;
mod rules;
mod service;

#[cfg(test)]
#[path = "affected_scope_integration_tests.rs"]
mod affected_scope_integration_tests;
#[cfg(test)]
#[path = "dependency_tour_integration_tests.rs"]
mod dependency_tour_integration_tests;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
