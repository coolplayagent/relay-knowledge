use std::collections::BTreeMap;

use super::{parse_flask_decorator, parse_flask_methods_decorator, parse_python_router_prefix};

#[test]
fn rejects_decorators_without_call_arguments() {
    assert!(parse_flask_decorator("@app.route", &BTreeMap::new()).is_none());
    assert!(parse_flask_methods_decorator("@app.methods").is_none());
}

#[test]
fn accepts_blueprints_and_rejects_unrecognized_router_factories() {
    assert!(
        parse_python_router_prefix("api = Blueprint('api', __name__, url_prefix='/v1')").is_some()
    );
    assert!(parse_python_router_prefix("api = NotARouter()").is_none());
}
