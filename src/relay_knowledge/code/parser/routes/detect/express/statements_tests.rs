use super::{express_route_statement, express_use_statement};

#[test]
fn direct_registration_closes_after_nested_calls_and_quoted_parentheses() {
    let lines = strings(&[
        "app.get(",
        "'/users',",
        "(request, response) => response.send(')')",
        ");",
        "app.post('/later', createUser);",
    ]);

    let statement = express_route_statement(&lines, 0);

    assert_eq!(
        statement,
        "app.get( '/users', (request, response) => response.send(')') );"
    );
}

#[test]
fn route_chain_stops_before_the_next_non_chain_statement() {
    let lines = strings(&[
        "router.route('/users')",
        ".get(listUsers)",
        ".post(createUser);",
        "router.delete('/later', deleteUser);",
    ]);

    let statement = express_route_statement(&lines, 0);

    assert_eq!(
        statement,
        "router.route('/users') .get(listUsers) .post(createUser);"
    );
}

#[test]
fn use_statement_closes_after_nested_router_arrays() {
    let lines = strings(&[
        "app.use(",
        "'/api',",
        "[usersRouter, auditRouter]",
        ");",
        "app.get('/later', later);",
    ]);

    let statement = express_use_statement(&lines, 0);

    assert_eq!(statement, "app.use( '/api', [usersRouter, auditRouter] );");
}

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}
