//! TypeScript, TSX and JavaScript module specifiers.
//!
//! A relative specifier (`./x`, `../x`) is resolved against the directory of the
//! importing file and probed the way the runtimes do: the path itself, then each
//! source extension, then a directory's index file. A bare specifier (`react`,
//! `@scope/pkg`) names a package, which is never a file in this graph.

use super::{join, relative_to, PathIndex};

const FAMILY: &str = "ecmascript";

/// Extensions tried after the bare path, in order.
const EXTENSIONS: [&str; 6] = [".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];

/// Index files tried inside a directory, in order.
const INDEX_FILES: [&str; 4] = ["index.ts", "index.tsx", "index.js", "index.jsx"];

/// Runtime extensions that TypeScript sources are imported under
/// (`./a.js` names `a.ts`), each with the source extensions it may stand for.
const RUNTIME_EXTENSIONS: [(&str, [&str; 2]); 4] = [
    (".js", [".ts", ".tsx"]),
    (".jsx", [".tsx", ".ts"]),
    (".mjs", [".mts", ".ts"]),
    (".cjs", [".cts", ".ts"]),
];

pub(super) fn module_files(paths: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    let relative = matches!(spec, "." | "..") || spec.starts_with("./") || spec.starts_with("../");
    if !relative {
        return Vec::new();
    }
    match relative_to(from, spec) {
        Some(base) => paths.first_exact(FAMILY, &probes(&base)),
        None => Vec::new(),
    }
}

/// Every spelling `base` can take as a file, in the order they are tried.
fn probes(base: &str) -> Vec<String> {
    let mut probes = Vec::new();
    if !base.is_empty() {
        probes.push(base.to_string());
        probes.extend(EXTENSIONS.iter().map(|ext| format!("{base}{ext}")));
    }
    probes.extend(INDEX_FILES.iter().map(|index| join(base, index)));
    for (runtime, sources) in RUNTIME_EXTENSIONS {
        if let Some(stem) = base.strip_suffix(runtime) {
            probes.extend(sources.iter().map(|ext| format!("{stem}{ext}")));
        }
    }
    probes
}
