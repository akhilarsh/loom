//! An `impl` scopes its members under the base name of its self type,
//! however that type is written: generic, with lifetimes, as a trait impl, or
//! through a path.

use std::path::Path;

use super::*;
use crate::context::source_graph::{EdgeProvenance, SourceEdgeKind, RECEIVER_CONFIDENCE};

/// The same members under every way of writing the self type `Foo`.
const IMPL_HEADERS: &[&str] = &[
    "impl<'a> Foo<'a>",
    "impl<T> Foo<T>",
    "impl<T: Clone> Show for Foo<T>",
    "impl crate::a::Foo",
    "impl<T> crate::a::Foo<T>",
];

fn extract(header: &str) -> FileExtraction {
    let source = format!(
        "{header} {{\n    fn helper(&self) {{}}\n    fn run(&self) {{\n        \
         self.helper();\n        Self::helper(self);\n    }}\n}}\n"
    );
    RustExtractor::new()
        .extract(Path::new("src/g.rs"), source.as_bytes())
        .unwrap()
}

#[test]
fn every_self_type_spelling_scopes_the_impl_and_its_methods_as_the_base_name() {
    for header in IMPL_HEADERS {
        let extraction = extract(header);
        let mut ids: Vec<&str> = extraction.nodes.iter().map(|n| n.id.as_str()).collect();
        ids.sort_unstable();

        assert_eq!(
            ids,
            vec![
                "src/g.rs",
                "src/g.rs#function:Foo::helper",
                "src/g.rs#function:Foo::run",
                "src/g.rs#implementation:Foo",
            ],
            "{header}"
        );
    }
}

/// The one `Calls` edge naming `symbol` in `source`, extracted as `src/g.rs`.
fn qualified_call(source: &str, symbol: &str) -> crate::context::source_graph::SourceEdge {
    let extraction = RustExtractor::new()
        .extract(Path::new("src/g.rs"), source.as_bytes())
        .unwrap();
    let calls: Vec<_> = extraction
        .edges
        .into_iter()
        .filter(|edge| edge.kind == SourceEdgeKind::Calls && edge.symbol == symbol)
        .collect();
    assert_eq!(calls.len(), 1, "calls to {symbol}: {calls:#?}");
    calls.into_iter().next().unwrap()
}

#[test]
fn a_qualified_call_on_a_type_only_impl_blocks_carry_is_left_to_the_resolver() {
    for header in [
        "impl From<Name> for String",
        "impl<T: Clone> Show for Vec<T>",
    ] {
        let owner = if header.ends_with("String") {
            "String"
        } else {
            "Vec"
        };
        let source = format!(
            "pub struct Name(pub String);\n{header} {{\n    fn from(n: Name) -> Self {{ todo!() }}\n}}\n\
             pub fn greeting() {{\n    {owner}::from(\"hi\");\n}}\n"
        );
        let edge = qualified_call(&source, &format!("{owner}::from"));

        assert_eq!(edge.from, "src/g.rs#function:greeting", "{header}");
        assert_eq!(
            edge.provenance,
            EdgeProvenance::Syntax,
            "{header}: {edge:#?}"
        );
        assert!(edge.is_unresolved(), "{header}: {edge:#?}");
        assert!(edge.candidates.is_empty(), "{header}: {edge:#?}");
    }
}

#[test]
fn a_qualified_call_on_a_type_this_file_defines_binds_its_impl_member() {
    let edge = qualified_call(
        "struct Widget;\nimpl Widget {\n    fn new() -> Self { Widget }\n}\n\
         fn make() {\n    Widget::new();\n}\n",
        "Widget::new",
    );

    assert_eq!(edge.from, "src/g.rs#function:make");
    assert_eq!(edge.to, "src/g.rs#function:Widget::new", "edge: {edge:#?}");
    assert_eq!(
        edge.provenance,
        EdgeProvenance::LocalName,
        "edge: {edge:#?}"
    );
}

#[test]
fn self_and_self_type_calls_in_a_generic_impl_bind_by_receiver() {
    for header in IMPL_HEADERS {
        let extraction = extract(header);
        let calls: Vec<_> = extraction
            .edges
            .iter()
            .filter(|edge| edge.kind == SourceEdgeKind::Calls && edge.symbol == "helper")
            .collect();

        let mut receivers: Vec<Option<&str>> =
            calls.iter().map(|edge| edge.receiver.as_deref()).collect();
        receivers.sort_unstable();
        assert_eq!(
            receivers,
            [Some("Self"), Some("self")],
            "{header}: {calls:#?}"
        );
        for edge in calls {
            assert_eq!(edge.from, "src/g.rs#function:Foo::run", "{header}");
            assert_eq!(edge.to, "src/g.rs#function:Foo::helper", "{header}");
            assert_eq!(edge.provenance, EdgeProvenance::Receiver, "{header}");
            assert_eq!(edge.confidence, RECEIVER_CONFIDENCE, "{header}");
        }
    }
}
