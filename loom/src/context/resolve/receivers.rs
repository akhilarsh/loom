//! Rule 3: a call on the enclosing type's own receiver (`self.m()`,
//! `this.m()`, `Self::m()`), where the type is declared in more than one file —
//! Rust `impl` blocks, C# `partial` classes, Ruby reopened classes — so the
//! member may live in a file the extractor never saw.

use std::collections::BTreeSet;

use crate::context::source_graph::{EdgeProvenance, SourceNodeKind};

use super::rules::{Outcome, Site};

/// Bind a self-receiver call to the one member of the enclosing type named like
/// the call, in the edge's family. Two or more members become candidates. When
/// no enclosing type is known, or it has no such member anywhere (an inherited
/// method), the call is treated like any other member call: definitions named
/// like it become candidates and nothing binds.
pub(super) fn self_call(site: &Site, keys: &mut BTreeSet<String>) -> Outcome {
    let member = site.edge.symbol.as_str();
    let Some(type_name) = enclosing_type(site) else {
        return site.name_candidates(member, keys);
    };
    let ids = site
        .symbols()
        .members(site.family(), type_name, member, keys);
    if ids.is_empty() {
        return site.name_candidates(member, keys);
    }
    site.decide(ids, EdgeProvenance::Receiver)
}

/// The last name segment of the type enclosing the edge's origin: the innermost
/// proper prefix of the origin's scope that a `Type`, `Implementation` or
/// `Interface` node of the same file declares.
///
/// Only that last segment is matched across files, so `impl Widget` at the top
/// of one file and `example::Widget` inside an inline module of another are
/// the same type.
fn enclosing_type<'a>(site: &Site<'a>) -> Option<&'a str> {
    let nodes = &site.entry.nodes;
    let origin = nodes.iter().find(|node| node.id == site.edge.from)?;
    let declares_type = |prefix: &[String]| {
        nodes.iter().any(|node| {
            matches!(
                node.kind,
                SourceNodeKind::Type | SourceNodeKind::Implementation | SourceNodeKind::Interface
            ) && node.scope.as_slice() == prefix
        })
    };
    (1..origin.scope.len())
        .rev()
        .map(|len| &origin.scope[..len])
        .find(|prefix| declares_type(prefix))
        .and_then(|prefix| prefix.last())
        .map(String::as_str)
}
