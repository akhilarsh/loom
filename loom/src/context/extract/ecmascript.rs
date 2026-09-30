//! Query text the ECMAScript dialects share. The TSX and JavaScript grammars
//! name their JSX nodes alike, so both extractors append the same patterns.

/// The JSX element patterns shared by the TSX and JavaScript extractors. A
/// capitalized element name is a component use and becomes a reference;
/// lowercase intrinsic tags (`div`) are not references. A member element
/// (`<ui.Icon />`) references its last segment. The closing tag repeats the
/// opening name, so it is not captured.
macro_rules! jsx_patterns {
    () => {
        r#"
(jsx_self_closing_element
  name: (identifier) @reference.name
  (#match? @reference.name "^[A-Z]"))

(jsx_opening_element
  name: (identifier) @reference.name
  (#match? @reference.name "^[A-Z]"))

(jsx_self_closing_element
  name: (member_expression
    property: (property_identifier) @reference.name)
  (#match? @reference.name "^[A-Z]"))

(jsx_opening_element
  name: (member_expression
    property: (property_identifier) @reference.name)
  (#match? @reference.name "^[A-Z]"))
"#
    };
}
pub(super) use jsx_patterns;
