//! JavaScript extraction, plus the assertion helpers the TSX tests reuse.

use std::path::Path;

use crate::context::extract::{FileExtraction, SourceGraphExtractor};
use crate::context::source_graph::{
    syntax_confidence, EdgeProvenance, FileCoverage, ImportBinding, SourceEdgeKind,
    LOCAL_NAME_CONFIDENCE, RECEIVER_CONFIDENCE, UNRESOLVED_TARGET,
};

use super::JavaScriptExtractor;

const CLASSES: &str = include_str!("../../../../tests/fixtures/source/javascript/classes.js");
const COMMONJS: &str = include_str!("../../../../tests/fixtures/source/javascript/commonjs.js");
const COMPONENT: &str = include_str!("../../../../tests/fixtures/source/javascript/component.jsx");
const SYNTAX_ERROR: &str =
    include_str!("../../../../tests/fixtures/source/javascript/syntax_error.js");

pub(in crate::context::extract) fn extract(
    extractor: &dyn SourceGraphExtractor,
    path: &str,
    source: &str,
) -> FileExtraction {
    extractor
        .extract(Path::new(path), source.as_bytes())
        .unwrap()
}

pub(in crate::context::extract) fn node_ids(extraction: &FileExtraction) -> Vec<&str> {
    let mut ids: Vec<&str> = extraction.nodes.iter().map(|n| n.id.as_str()).collect();
    ids.sort_unstable();
    ids
}

/// One edge the test expects, with its provenance, confidence, receiver and site line.
pub(in crate::context::extract) struct Want {
    kind: SourceEdgeKind,
    from: String,
    to: String,
    symbol: String,
    provenance: EdgeProvenance,
    receiver: Option<String>,
    line: usize,
}

impl Want {
    /// An edge from `from` to `to` (`UNRESOLVED_TARGET` when the file cannot bind it).
    pub(in crate::context::extract) fn new(
        kind: SourceEdgeKind,
        (from, to): (&str, &str),
        symbol: &str,
        provenance: EdgeProvenance,
        line: usize,
    ) -> Self {
        Want {
            kind,
            from: from.to_string(),
            to: to.to_string(),
            symbol: symbol.to_string(),
            provenance,
            receiver: None,
            line,
        }
    }

    /// The receiver text the edge must carry.
    pub(in crate::context::extract) fn on(mut self, receiver: &str) -> Self {
        self.receiver = Some(receiver.to_string());
        self
    }

    /// Asserts exactly one edge matches `from`, `to`, `kind`, `symbol` and receiver, and that it
    /// carries the expected evidence. An unbound edge has no candidates.
    pub(in crate::context::extract) fn check(&self, extraction: &FileExtraction) {
        let found: Vec<_> = extraction
            .edges
            .iter()
            .filter(|e| {
                e.from == self.from
                    && e.to == self.to
                    && e.kind == self.kind
                    && e.symbol == self.symbol
                    && e.receiver == self.receiver
            })
            .collect();
        assert_eq!(
            found.len(),
            1,
            "edges for {}: {:#?}",
            self.symbol,
            extraction.edges
        );
        let edge = found[0];

        let confidence = match self.provenance {
            EdgeProvenance::Receiver => RECEIVER_CONFIDENCE,
            EdgeProvenance::LocalName => LOCAL_NAME_CONFIDENCE,
            _ => syntax_confidence(self.kind),
        };
        assert_eq!(edge.provenance, self.provenance, "edge: {edge:#?}");
        assert_eq!(edge.confidence, confidence, "edge: {edge:#?}");
        assert_eq!(edge.receiver, self.receiver, "edge: {edge:#?}");
        assert_eq!(
            edge.sites.iter().map(|s| s.line_start).collect::<Vec<_>>(),
            vec![self.line],
            "edge: {edge:#?}"
        );
        assert!(edge.candidates.is_empty(), "edge: {edge:#?}");
    }
}

/// `(name, alias, glob)` of every binding for `path`, in source order.
pub(in crate::context::extract) fn bindings_for<'a>(
    extraction: &'a FileExtraction,
    path: &str,
) -> Vec<(Option<&'a str>, Option<&'a str>, bool)> {
    let mut found: Vec<&ImportBinding> = extraction
        .imports
        .iter()
        .filter(|b| b.path == path)
        .collect();
    found.sort_by_key(|b| (b.site.start_byte, b.name.clone()));
    found
        .into_iter()
        .map(|b| (b.name.as_deref(), b.alias.as_deref(), b.glob))
        .collect()
}

pub(in crate::context::extract) fn references_to(
    extraction: &FileExtraction,
    symbol: &str,
) -> usize {
    extraction
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::References && e.symbol == symbol)
        .count()
}

fn javascript(path: &str, source: &str) -> FileExtraction {
    extract(&JavaScriptExtractor::new(), path, source)
}

#[test]
fn classes_fixture_yields_declarations_with_distinct_same_name_methods() {
    let extraction = javascript("src/classes.js", CLASSES);

    assert_eq!(extraction.coverage, FileCoverage::Full);
    assert_eq!(
        node_ids(&extraction),
        vec![
            "src/classes.js",
            "src/classes.js#constant:LIMIT",
            "src/classes.js#constant:config",
            "src/classes.js#function:Runner::helper",
            "src/classes.js#function:Runner::run",
            "src/classes.js#function:Runner::start",
            "src/classes.js#function:Worker::run",
            "src/classes.js#function:handler",
            "src/classes.js#function:legacy",
            "src/classes.js#function:run",
            "src/classes.js#type:Runner",
            "src/classes.js#type:Worker",
        ]
    );
}

/// A `Calls` edge between two ids of `src/classes.js`, each given by its fragment.
fn call(from: &str, to: &str, symbol: &str, provenance: EdgeProvenance, line: usize) -> Want {
    let id = |fragment: &str| match fragment {
        UNRESOLVED_TARGET => fragment.to_string(),
        _ => format!("src/classes.js#{fragment}"),
    };
    Want::new(
        SourceEdgeKind::Calls,
        (&id(from), &id(to)),
        symbol,
        provenance,
        line,
    )
}

#[test]
fn this_calls_bind_to_the_enclosing_class_member() {
    let extraction = javascript("src/classes.js", CLASSES);

    call(
        "function:Runner::start",
        "function:Runner::run",
        "run",
        EdgeProvenance::Receiver,
        11,
    )
    .on("this")
    .check(&extraction);
    call(
        "function:Runner::run",
        "function:Runner::helper",
        "helper",
        EdgeProvenance::Receiver,
        17,
    )
    .on("this")
    .check(&extraction);
}

#[test]
fn bare_calls_skip_members_and_bind_to_the_module_function() {
    let extraction = javascript("src/classes.js", CLASSES);

    call(
        "function:Runner::start",
        "function:run",
        "run",
        EdgeProvenance::LocalName,
        12,
    )
    .check(&extraction);
    call(
        "function:handler",
        "function:run",
        "run",
        EdgeProvenance::LocalName,
        30,
    )
    .check(&extraction);
}

#[test]
fn dynamic_receivers_and_imported_names_stay_unresolved() {
    let extraction = javascript("src/classes.js", CLASSES);

    call(
        "function:Worker::run",
        UNRESOLVED_TARGET,
        "run",
        EdgeProvenance::Syntax,
        25,
    )
    .on("obj")
    .check(&extraction);
    call(
        "function:Runner::start",
        UNRESOLVED_TARGET,
        "b",
        EdgeProvenance::Syntax,
        13,
    )
    .check(&extraction);
}

#[test]
fn imports_aliases_and_reexports_keep_their_bindings() {
    let extraction = javascript("src/classes.js", CLASSES);

    assert_eq!(
        bindings_for(&extraction, "./alias"),
        vec![(Some("a"), Some("b"), false)]
    );
    assert_eq!(
        bindings_for(&extraction, "./everything"),
        vec![(None, None, true)]
    );
    assert_eq!(
        bindings_for(&extraction, "./other"),
        vec![(Some("c"), Some(""), false)]
    );
    let imports = extraction
        .edges
        .iter()
        .filter(|e| e.kind == SourceEdgeKind::Imports)
        .count();
    assert_eq!(imports, 3, "edges: {:#?}", extraction.edges);
}

#[test]
fn require_is_an_import_binding() {
    let extraction = javascript("src/commonjs.js", COMMONJS);

    assert_eq!(extraction.coverage, FileCoverage::Full);
    assert_eq!(
        bindings_for(&extraction, "./util"),
        vec![(None, Some("util"), false)]
    );
    assert_eq!(
        bindings_for(&extraction, "./codec"),
        vec![
            (Some("format"), Some("fmt"), false),
            (Some("parse"), Some("parse"), false)
        ]
    );
    assert_eq!(
        bindings_for(&extraction, "./polyfill"),
        vec![(None, Some(""), false)]
    );
    assert_eq!(
        bindings_for(&extraction, "./importer"),
        vec![(None, Some("importer"), false)]
    );
}

#[test]
fn require_yields_one_import_edge_and_no_call_edge() {
    let extraction = javascript("src/commonjs.js", COMMONJS);

    for path in ["./util", "./codec", "./polyfill", "./importer"] {
        let count = extraction
            .edges
            .iter()
            .filter(|e| e.kind == SourceEdgeKind::Imports && e.symbol == path)
            .count();
        assert_eq!(count, 1, "{path}: {:#?}", extraction.edges);
    }
    assert!(
        !extraction.edges.iter().any(|e| e.symbol == "require"),
        "edges: {:#?}",
        extraction.edges
    );
}

#[test]
fn required_names_keep_their_receiver_on_calls() {
    let extraction = javascript("src/commonjs.js", COMMONJS);

    // `util.parse()` keeps its receiver; the destructured `parse()` has none.
    let run = "src/commonjs.js#function:run";
    Want::new(
        SourceEdgeKind::Calls,
        (run, UNRESOLVED_TARGET),
        "parse",
        EdgeProvenance::Syntax,
        7,
    )
    .on("util")
    .check(&extraction);
    Want::new(
        SourceEdgeKind::Calls,
        (run, UNRESOLVED_TARGET),
        "parse",
        EdgeProvenance::Syntax,
        8,
    )
    .check(&extraction);
}

#[test]
fn jsx_components_become_reference_edges_and_intrinsic_tags_do_not() {
    let extraction = javascript("src/component.jsx", COMPONENT);

    assert_eq!(extraction.coverage, FileCoverage::Full);
    assert_eq!(
        node_ids(&extraction),
        vec!["src/component.jsx", "src/component.jsx#function:App"]
    );
    for (symbol, line) in [("Layout", 6), ("Button", 7), ("Icon", 9)] {
        Want::new(
            SourceEdgeKind::References,
            ("src/component.jsx#function:App", UNRESOLVED_TARGET),
            symbol,
            EdgeProvenance::Syntax,
            line,
        )
        .check(&extraction);
    }
    assert_eq!(references_to(&extraction, "span"), 0);
    assert_eq!(
        bindings_for(&extraction, "./Layout"),
        vec![(Some("default"), Some("Layout"), false)]
    );
}

#[test]
fn a_reexport_records_the_name_it_exports() {
    let extraction = javascript("src/classes.js", CLASSES);
    let exported = |path: &str| {
        let bindings = extraction.imports.iter().filter(|b| b.path == path);
        bindings
            .map(|b| b.exported_as.as_deref())
            .collect::<Vec<_>>()
    };

    assert_eq!(exported("./other"), [Some("d")]);
    assert_eq!(exported("./alias"), [None]);
    assert_eq!(exported("./everything"), [None]);
}

#[test]
fn syntax_errors_keep_only_the_file_node() {
    let extraction = javascript("src/broken.js", SYNTAX_ERROR);

    assert_eq!(extraction.coverage.status(), "parse-error");
    assert_eq!(extraction.nodes.len(), 1);
    assert!(extraction.edges.is_empty());
}
