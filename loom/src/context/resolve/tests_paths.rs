//! Module specs as written, matched onto files by the convention of the
//! importing file's dialect. Graphs are built by hand, so the Java, C#, PHP, Ruby
//! and C cases do not depend on an extractor existing for them.

use std::collections::BTreeSet;
use std::path::Path;

use super::fixtures::*;
use super::paths::{import_candidates, PathIndex};
use crate::context::extract::dialect::dialect_for_path;
use crate::context::source_graph::SourceNodeKind;

/// An index over empty files at `paths`.
fn index_of(paths: &[&str]) -> PathIndex {
    let files = paths
        .iter()
        .map(|path| (*path, source_file(path, &[], vec![])))
        .collect();
    PathIndex::build(&graph_of(files))
}

/// An index over files that each declare the given namespaces, in the
/// single-segment `Module` shape language extractors emit.
fn namespaced(files: &[(&str, &[&str])]) -> PathIndex {
    let entries = files
        .iter()
        .map(|(path, namespaces)| {
            let mut entry = source_file(path, &[], vec![]);
            for namespace in *namespaces {
                entry
                    .nodes
                    .push(scoped_node(path, SourceNodeKind::Module, namespace));
            }
            (*path, entry)
        })
        .collect();
    PathIndex::build(&graph_of(entries))
}

/// Files `spec` names from `from`, through the dialect of `from`.
fn resolve(index: &PathIndex, spec: &str, from: &str) -> Vec<String> {
    import_candidates(spec, from, index, &mut BTreeSet::new())
}

fn package(index: &PathIndex, from: &str) -> Vec<String> {
    let dialect = dialect_for_path(Path::new(from)).expect("a known dialect");
    index.package_files(from, dialect, &mut BTreeSet::new())
}

#[test]
fn typescript_relative_specifier_resolves_and_bare_one_is_external() {
    let index = index_of(&["src/util.ts", "src/main.ts", "src/myutil.ts"]);

    assert_eq!(resolve(&index, "./util", "src/main.ts"), ["src/util.ts"]);
    assert!(resolve(&index, "react", "src/main.ts").is_empty());
    assert!(resolve(&index, "@scope/pkg", "src/main.ts").is_empty());
}

#[test]
fn tsx_relative_specifier_resolves_to_a_tsx_file() {
    let index = index_of(&["src/Button.tsx", "src/App.tsx"]);

    assert_eq!(
        resolve(&index, "./Button", "src/App.tsx"),
        ["src/Button.tsx"]
    );
}

#[test]
fn javascript_parent_specifier_resolves_to_a_directory_index() {
    let index = index_of(&["lib/index.js", "src/main.js"]);

    assert_eq!(resolve(&index, "../lib", "src/main.js"), ["lib/index.js"]);
}

#[test]
fn a_specifier_climbing_above_the_root_is_external() {
    let index = index_of(&["util.ts", "main.ts"]);

    assert!(resolve(&index, "../util", "main.ts").is_empty());
}

#[test]
fn a_runtime_extension_names_the_typescript_source() {
    let index = index_of(&["src/util.ts", "src/main.ts"]);

    assert_eq!(resolve(&index, "./util.js", "src/main.ts"), ["src/util.ts"]);
}

#[test]
fn a_verbatim_asset_probe_never_matches_outside_the_family() {
    let index = index_of(&["src/styles.css", "src/util.py", "src/main.ts"]);

    assert!(resolve(&index, "./styles.css", "src/main.ts").is_empty());
    assert!(resolve(&index, "./util", "src/main.ts").is_empty());
}

#[test]
fn a_file_with_no_dialect_resolves_nothing() {
    let index = index_of(&["src/util.ts", "notes.txt"]);

    assert!(resolve(&index, "./util", "notes.txt").is_empty());
}

#[test]
fn python_relative_and_dotted_specs_resolve() {
    let index = index_of(&[
        "app/views.py",
        "app/models.py",
        "app/sub/__init__.py",
        "lib/tools/io.py",
        "top.py",
    ]);

    assert_eq!(
        resolve(&index, ".models", "app/views.py"),
        ["app/models.py"]
    );
    assert_eq!(
        resolve(&index, ".sub", "app/views.py"),
        ["app/sub/__init__.py"]
    );
    assert_eq!(resolve(&index, "..top", "app/views.py"), ["top.py"]);
    assert_eq!(
        resolve(&index, "tools.io", "app/views.py"),
        ["lib/tools/io.py"]
    );
    assert!(resolve(&index, "..oops", "top.py").is_empty());
    assert!(resolve(&index, "requests.adapters", "app/views.py").is_empty());
}

#[test]
fn go_import_path_resolves_to_every_file_of_the_package_directory() {
    let index = index_of(&[
        "pkg/util/b.go",
        "pkg/util/a.go",
        "pkg/util/notes.md",
        "pkg/other/c.go",
        "main.go",
    ]);

    assert_eq!(
        resolve(&index, "example.com/m/pkg/util", "main.go"),
        ["pkg/util/a.go", "pkg/util/b.go"]
    );
    assert!(resolve(&index, "example.com/m/pkg/missing", "main.go").is_empty());
}

#[test]
fn a_hostless_go_path_never_matches_a_namesake_directory() {
    let index = index_of(&["internal/http/h.go", "main.go"]);

    assert!(resolve(&index, "net/http", "main.go").is_empty());
    assert_eq!(
        resolve(&index, "internal/http", "main.go"),
        ["internal/http/h.go"]
    );
}

#[test]
fn java_class_and_glob_imports_resolve() {
    let index = index_of(&[
        "src/main/java/a/b/C.java",
        "src/main/java/a/b/D.java",
        "src/main/java/xa/b/C.java",
        "src/main/java/a/Main.java",
    ]);
    let from = "src/main/java/a/Main.java";

    assert_eq!(resolve(&index, "a.b.C", from), ["src/main/java/a/b/C.java"]);
    assert_eq!(
        resolve(&index, "a.b.*", from),
        ["src/main/java/a/b/C.java", "src/main/java/a/b/D.java"]
    );
    assert_eq!(
        resolve(&index, "a.b.C.helper", from),
        ["src/main/java/a/b/C.java"]
    );
    assert!(resolve(&index, "java.util.List", from).is_empty());
}

#[test]
fn csharp_using_resolves_to_the_files_declaring_the_namespace() {
    let index = namespaced(&[
        ("src/A.cs", &["A.B"]),
        ("lib/B.cs", &["A.B"]),
        ("src/C.cs", &["C"]),
    ]);

    assert_eq!(resolve(&index, "A.B", "src/C.cs"), ["lib/B.cs", "src/A.cs"]);
    assert!(resolve(&index, "System.Linq", "src/C.cs").is_empty());
}

#[test]
fn ruby_require_relative_resolves_against_the_importing_directory() {
    let index = index_of(&["lib/x.rb", "app/main.rb", "app/x.rb"]);

    assert_eq!(resolve(&index, "../lib/x", "app/main.rb"), ["lib/x.rb"]);
    assert!(resolve(&index, "./missing", "app/main.rb").is_empty());
}

#[test]
fn ruby_require_and_require_relative_pick_different_files() {
    let index = index_of(&["app/x.rb", "lib/x.rb", "app/main.rb"]);

    assert_eq!(resolve(&index, "./x", "app/main.rb"), ["app/x.rb"]);
    assert_eq!(resolve(&index, "x", "app/main.rb"), ["lib/x.rb"]);
}

#[test]
fn a_bare_ruby_spec_without_a_lib_file_matches_every_suffix() {
    let index = index_of(&["a/util.rb", "b/util.rb", "b/myutil.rb", "main.rb"]);

    assert_eq!(
        resolve(&index, "util", "main.rb"),
        ["a/util.rb", "b/util.rb"]
    );
}

#[test]
fn php_use_resolves_by_psr4_suffix_and_falls_back_to_the_namespace() {
    let index = namespaced(&[
        ("src/App/Models/User.php", &["App.Models"]),
        ("app/Models/Post.php", &["App.Models"]),
        ("src/index.php", &[]),
    ]);
    let from = "src/index.php";

    assert_eq!(
        resolve(&index, "App\\Models\\User", from),
        ["src/App/Models/User.php"]
    );
    assert_eq!(
        resolve(&index, "App\\Models\\Post", from),
        ["app/Models/Post.php"]
    );
    assert_eq!(
        resolve(&index, "App\\Models", from),
        ["app/Models/Post.php", "src/App/Models/User.php"]
    );
    assert!(resolve(&index, "Vendor\\Lib\\Thing", from).is_empty());
}

#[test]
fn php_require_paths_resolve_relative_to_the_including_file() {
    let index = index_of(&["src/lib/x.php", "src/index.php", "other/lib/x.php"]);

    assert_eq!(
        resolve(&index, "./lib/x.php", "src/index.php"),
        ["src/lib/x.php"]
    );
    assert_eq!(
        resolve(&index, "lib/x.php", "src/index.php"),
        ["src/lib/x.php"]
    );
}

#[test]
fn c_include_resolves_relative_first_then_by_suffix_and_system_headers_are_external() {
    let index = index_of(&["src/main.c", "src/util.h", "include/net/sock.h"]);

    assert_eq!(resolve(&index, "util.h", "src/main.c"), ["src/util.h"]);
    assert_eq!(
        resolve(&index, "net/sock.h", "src/main.c"),
        ["include/net/sock.h"]
    );
    assert!(resolve(&index, "<stdio.h>", "src/main.c").is_empty());
}

#[test]
fn a_header_named_by_a_cpp_file_matches_across_the_c_family() {
    let index = index_of(&["src/widget.cpp", "src/widget.h"]);

    assert_eq!(
        resolve(&index, "widget.h", "src/widget.cpp"),
        ["src/widget.h"]
    );
}

#[test]
fn package_files_share_a_directory_for_go_and_java() {
    let index = index_of(&[
        "pkg/a.go",
        "pkg/b.go",
        "pkg/readme.md",
        "other/c.go",
        "j/A.java",
        "j/B.java",
        "j/a.go",
    ]);

    assert_eq!(package(&index, "pkg/a.go"), ["pkg/b.go"]);
    assert_eq!(package(&index, "j/A.java"), ["j/B.java"]);
}

#[test]
fn package_files_share_a_namespace_across_directories_for_csharp() {
    let index = namespaced(&[
        ("src/A.cs", &["App.Core"]),
        ("lib/B.cs", &["App.Core"]),
        ("src/C.cs", &["App.Other"]),
    ]);

    assert_eq!(package(&index, "src/A.cs"), ["lib/B.cs"]);
    assert!(package(&index, "src/C.cs").is_empty());
}

#[test]
fn namespace_files_lists_the_declaring_files_of_a_family() {
    let index = namespaced(&[("a/A.java", &["x.y"]), ("b/B.cs", &["x.y"])]);
    let mut keys = BTreeSet::new();

    assert_eq!(
        index.namespace_files("java", "x.y", &mut keys),
        ["a/A.java"]
    );
    assert!(index.namespace_files("java", "x.z", &mut keys).is_empty());
}

#[test]
fn a_rust_path_naming_an_external_crate_never_matches_a_local_namesake() {
    let index = index_of(&["src/lib.rs", "src/x/de.rs", "src/app.rs"]);

    assert!(resolve(&index, "serde::de::*", "src/app.rs").is_empty());
    assert!(resolve(&index, "serde::de::Deserialize", "src/app.rs").is_empty());
}

#[test]
fn a_rust_path_naming_a_local_module_resolves_without_an_anchor() {
    let index = index_of(&[
        "src/lib.rs",
        "src/codex.rs",
        "src/app/inner.rs",
        "src/app.rs",
    ]);

    assert_eq!(
        resolve(&index, "codex::run", "src/app.rs"),
        ["src/codex.rs"]
    );
    assert_eq!(
        resolve(&index, "inner::Item", "src/app.rs"),
        ["src/app/inner.rs"]
    );
}

#[test]
fn rust_anchored_paths_keep_their_anchors() {
    let index = index_of(&[
        "src/lib.rs",
        "src/skills/mod.rs",
        "src/skills/install.rs",
        "src/skills/catalog.rs",
        "src/other/catalog.rs",
    ]);

    assert_eq!(
        resolve(&index, "crate::skills::catalog::{a, b}", "src/lib.rs"),
        ["src/skills/catalog.rs"]
    );
    assert_eq!(
        resolve(&index, "super::catalog::Item", "src/skills/install.rs"),
        ["src/skills/catalog.rs"]
    );
    assert_eq!(
        resolve(&index, "crate::skills", "src/lib.rs"),
        ["src/skills/mod.rs"]
    );
}

#[test]
fn lookups_record_the_keys_they_consulted() {
    let index = namespaced(&[("src/A.cs", &["A.B"]), ("src/m.ts", &[])]);
    let mut keys = BTreeSet::new();
    let csharp = dialect_for_path(Path::new("src/A.cs")).expect("csharp");
    let ecmascript = dialect_for_path(Path::new("src/m.ts")).expect("typescript");

    index.module_files("A.B", "src/A.cs", csharp, &mut keys);
    assert_eq!(
        keys,
        BTreeSet::from(["ns:csharp:A.B".to_string(), "pathset:csharp".to_string()])
    );

    let mut keys = BTreeSet::new();
    index.module_files("./x", "src/m.ts", ecmascript, &mut keys);
    assert_eq!(keys, BTreeSet::from(["pathset:ecmascript".to_string()]));

    let mut keys = BTreeSet::new();
    index.package_files("src/A.cs", csharp, &mut keys);
    assert!(keys.contains("pathset:csharp"));
    assert!(keys.contains("ns:csharp:A.B"));
}
