use crate::model::Widget;
use crate::util::parse_id;

#[test]
fn widget_label_contains_id() {
    let widget = Widget::new(3, "Nut");
    assert!(widget.label().starts_with("3"));
}

#[test]
fn parse_id_rejects_text() {
    assert_eq!(parse_id("abc"), None);
}
