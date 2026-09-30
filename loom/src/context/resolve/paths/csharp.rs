//! C# `using` directives.
//!
//! `using A.B;` names a namespace, not a file, so it resolves through the
//! namespace index to every file declaring `A.B`.

use std::collections::BTreeSet;

use super::PathIndex;

pub(super) fn module_files(
    paths: &PathIndex,
    spec: &str,
    keys: &mut BTreeSet<String>,
) -> Vec<String> {
    paths.namespace_files("csharp", spec, keys)
}
