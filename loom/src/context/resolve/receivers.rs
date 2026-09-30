//! Rule 3: a call on the enclosing type's own receiver (`self.m()`,
//! `this.m()`, `Self::m()`).
//!
//! Only a dialect that declares one type across files looks beyond the edge's
//! own file — Rust `impl` blocks, C# `partial` classes, Ruby reopened classes,
//! C++ out-of-line definitions — because there the member may live in a file
//! the extractor never saw: within the type's crate (Rust) or namespace (C#),
//! anywhere in the family otherwise. In every other dialect a type is one
//! declaration, so a member its file does not hold is inherited, and
//! inheritance is not followed: a namesake type's member, in another file or
//! nested under another type of the same file, is never the enclosing type's.
//!
//! A PHP type's members also include those of every trait it uses: a
//! `use Loggable;` in a class body is a `References` edge from the class node
//! to `Loggable`, and the trait's methods are the class's own. No other dialect
//! reads a type's `References` edges as trait uses: a TSX class's JSX
//! `<Dialog/>` is one too, and `Dialog`'s methods are not the class's.

use std::collections::BTreeSet;

use crate::context::source_graph::{EdgeProvenance, SourceEdgeKind, SourceNode};

use super::rules::{Outcome, Site};
use super::symbols::{declares_members, file_of};

/// Dialects whose types are declared across files.
const SPLIT_TYPE_DIALECTS: [&str; 4] = ["rust", "csharp", "ruby", "cpp"];

/// The dialect whose types use traits by a `References` edge from their body.
const TRAIT_USE_DIALECT: &str = "php";

/// Bind a self-receiver call to the one member of the enclosing type named like
/// the call, in the edge's family; failing that, to the one member of a trait
/// the type uses. Two or more members become candidates. When no enclosing
/// type is known, or neither has such a member where its parts can be (an
/// inherited method), the call is treated like any other member call:
/// definitions named like it become candidates and nothing binds.
pub(super) fn self_call(site: &Site, keys: &mut BTreeSet<String>) -> Outcome {
    let member = site.edge.symbol.as_str();
    let Some(scope) = enclosing_type(site) else {
        return site.name_candidates(member, keys);
    };
    let symbols = site.symbols();
    let mut ids = own_members(site, scope, member, keys);
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

/// The members named `member` that can be parts of the enclosing type at
/// `scope`. A dialect that splits types across files matches the type by its
/// last name segment in every file that may share it. Every other dialect
/// matches the whole scope in the edge's own file, so `B.Meta`'s self call
/// never binds a member of the nested namesake `A.Meta`.
fn own_members(
    site: &Site,
    scope: &[String],
    member: &str,
    keys: &mut BTreeSet<String>,
) -> Vec<String> {
    let symbols = site.symbols();
    let family = site.family();
    if !SPLIT_TYPE_DIALECTS.contains(&site.dialect.id) {
        let ids = symbols.members(family, &scope.join("::"), member, keys);
        return ids
            .into_iter()
            .filter(|id| directly_under(site, id, scope))
            .collect();
    }
    let Some(type_name) = scope.last() else {
        return Vec::new();
    };
    let paths = &site.indexes.paths;
    symbols
        .members(family, type_name, member, keys)
        .into_iter()
        .filter(|id| paths.may_share_type(family, site.file, file_of(id), keys))
        .collect()
}

/// Whether `id` is a node of the edge's own file scoped exactly `scope` plus
/// its own name, rather than deeper under a type of the same trailing scope.
fn directly_under(site: &Site, id: &str, scope: &[String]) -> bool {
    site.entry.nodes.iter().any(|node| {
        node.id == id && node.scope.len() == scope.len() + 1 && node.scope.starts_with(scope)
    })
}

/// The scope of the type enclosing the edge's origin: the innermost proper
/// prefix of the origin's scope that a `Type`, `Implementation` or `Interface`
/// node of the same file declares.
///
/// Where a dialect splits types across files only its last segment is matched
/// there, so `impl Widget` at the top of one file and `example::Widget` inside
/// an inline module of another are the same type.
fn enclosing_type<'a>(site: &Site<'a>) -> Option<&'a [String]> {
    let nodes = &site.entry.nodes;
    let origin = nodes.iter().find(|node| node.id == site.edge.from)?;
    (1..origin.scope.len())
        .rev()
        .map(|len| &origin.scope[..len])
        .find(|prefix| nodes.iter().any(|node| declares(node, prefix)))
}

/// The last name segment of every trait a node of the edge's file declaring
/// the type at `scope` references from its own body. Empty outside PHP.
fn traits_used<'a>(site: &Site<'a>, scope: &[String]) -> Vec<&'a str> {
    if site.dialect.id != TRAIT_USE_DIALECT {
        return Vec::new();
    }
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
    declares_members(node.kind) && node.scope.as_slice() == scope
}
