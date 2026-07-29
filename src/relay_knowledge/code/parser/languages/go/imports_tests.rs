use super::import_specs;

#[test]
fn import_specs_ignore_quotes_inside_multiline_comments() {
    let specs = import_specs(
        r#"
import (
    "context"
    /*
       alias "example.com/commented"
       "example.com/also-commented"
    */
    named "example.com/used"
)
"#,
    );

    assert_eq!(specs, ["context", "named example.com/used"]);
}
