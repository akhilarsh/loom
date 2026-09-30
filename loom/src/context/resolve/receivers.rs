//! Rule 3: a call on the enclosing type's own receiver (`self.m()`,
//! `this.m()`, `Self::m()`), where the type is declared in more than one file —
//! Rust `impl` blocks, C# `partial` classes, Ruby reopened classes — so the
//! member may live in a file the extractor never saw.
//!
//! A type's members also include those of every trait it uses: a PHP
//! `use Loggable;` in a class body is a `References` edge from the class node
//! to `Loggable`, and the trait's methods are the class's own.

use std::collections::BTreeSet;

use crate::context::source_graph::{EdgeProvenance, SourceEdgeKind, SourceNode, SourceNodeKind};

use super::rules::{Outcome, Site};

/// Bind a self-receiver call to the one member of the enclosing type named like
/// the call, in the edge's family; failing that, to the one member of a trait
/// the type uses. Two or more members become candidates. When no enclosing
/// type is known, or neither has such a member anywhere (an inherited method),
/// the call is treated like any other member call: definitions named like it
/// become candidates and nothing binds.
pub(super) fn self_call(site: &Site, keys: &mut BTreeSet<String>) -> Outcome {
    let member = site.edge.symbol.as_str();
    let Some(scope) = enclosing_type(site) else {
        return site.name_candidates(member, keys);
    };
    let Some(type_name) = scope.last() else {
        return site.name_candidates(member, keys);
    };
    let symbols = site.symbols();
    let mut ids = symbols.members(site.family(), type_name, member, keys);
    if ids.is_empty() {
        for used in traits_used(site, scope) {
            ids.extend(symbols.members(site.family(), used, member, keys));
        }
    }
    if ids.is_empty() {
        return site.name_candidates(member, keys);
    }
    ids.sort();
    ids.dedup();
    site.decide(ids, EdgeProvenance::Receiver)
}

/// The scope of the type enclosing the edge's origin: the innermost proper
/// prefix of the origin's scope that a `Type`, `Implementation` or `Interface`
/// node of the same file declares.
///
/// Only its last segment is matched across files, so `impl Widget` at the top
/// of one file and `example::Widget` inside an inline module of another are
/// the same type.
fn enclosing_type<'a>(site: &Site<'a>) -> Option<&'a [String]> {
    let nodes = &site.entry.nodes;
    let origin = nodes.iter().find(|node| node.id == site.edge.from)?;
    (1..origin.scope.len())
        .rev()
        .map(|len| &origin.scope[..len])
        .find(|prefix| nodes.iter().any(|node| declares(node, prefix)))
}

/// The last name segment of every trait a node of the edge's file declaring
/// the type at `scope` references from its own body.
fn traits_used<'a>(site: &Site<'a>, scope: &[String]) -> Vec<&'a str> {
    let nodes = &site.entry.nodes;
    let declaring: Vec<&str> = nodes
        .iter()
        .filter(|node| declares(node, scope))
        .map(|node| node.id.as_str())
        .collect();
    site.entry
        .edges
        .iter()
        .filter(|edge| edge.kind == SourceEdgeKind::References)
        .filter(|edge| declaring.contains(&edge.from.as_str()))
        .filter_map(|edge| edge.symbol.rsplit(['\\', ':', '.']).next())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Whether `node` declares a type, implementation or interface at `scope`.
fn declares(node: &SourceNode, scope: &[String]) -> bool {
    matches!(
        node.kind,
        SourceNodeKind::Type | SourceNodeKind::Implementation | SourceNodeKind::Interface
    ) && node.scope.as_slice() == scope
}
