use super::*;

#[test]
fn canvas_selection_enables_only_its_owned_fact_families() {
    assert!(GraphCanvasSelection::Knowledge.includes_knowledge());
    assert!(!GraphCanvasSelection::Knowledge.includes_code());
    assert!(!GraphCanvasSelection::Code.includes_knowledge());
    assert!(GraphCanvasSelection::Code.includes_code());
    assert!(GraphCanvasSelection::Mixed.includes_knowledge());
    assert!(GraphCanvasSelection::Mixed.includes_code());
}
