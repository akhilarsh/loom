//! Ruby `require`, `require_relative` and `load` specs.
//!
//! A spec starting `./` or `../` is how `require_relative 'x'` arrives: it names a
//! file next to the importing one and resolves against that directory only. A bare
//! spec (`require`, `load`) is found on the load path: `lib/<spec>.rb` when that
//! file exists, else every `<spec>.rb` suffix match. The load path is not known,
//! so a one-segment spec naming a gem (`require 'json'`) lands on a project
//! file of that name when one exists: the same trade-off as a one-segment
//! Python import (`python::absolute`).

use super::{relative_to, PathIndex};

const FAMILY: &str = "ruby";

pub(super) fn module_files(paths: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    let file = with_extension(spec);
    if spec.starts_with("./") || spec.starts_with("../") {
        return match relative_to(from, &file) {
            Some(path) => paths.first_exact(FAMILY, &[path]),
            None => Vec::new(),
        };
    }
    let on_lib = paths.first_suffix(FAMILY, &[format!("lib/{file}")]);
    if !on_lib.is_empty() {
        return on_lib;
    }
    paths.first_suffix(FAMILY, &[file])
}

fn with_extension(spec: &str) -> String {
    if spec.ends_with(".rb") {
        return spec.to_string();
    }
    format!("{spec}.rb")
}
