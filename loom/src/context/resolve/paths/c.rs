//! C and C++ `#include` paths.
//!
//! `"x.h"` is resolved against the including file's directory first, then
//! suffix-matched against every C-family file. `<x>` is a system or third-party
//! header and is external.

use super::{relative_to, PathIndex};

const FAMILY: &str = "c";

pub(super) fn module_files(paths: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    if spec.starts_with('<') {
        return Vec::new();
    }
    if let Some(path) = relative_to(from, spec) {
        let found = paths.first_exact(FAMILY, &[path]);
        if !found.is_empty() {
            return found;
        }
    }
    paths.first_suffix(FAMILY, &[spec.trim_start_matches("./").to_string()])
}
