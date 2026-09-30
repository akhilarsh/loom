//! Grouping files by the nearest ancestor directory that holds a build
//! manifest.

use std::collections::BTreeSet;

/// File names that mark a directory as a subproject.
const MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "composer.json",
    "CMakeLists.txt",
    "meson.build",
];

/// Manifest name suffixes (`app.csproj`, `app.sln`).
const MANIFEST_SUFFIXES: &[&str] = &[".csproj", ".sln"];

/// Label of the fallback subproject: the checkout root.
pub(super) const ROOT_SUBPROJECT: &str = ".";

/// The directories of one checkout that hold a manifest.
pub(super) struct SubprojectIndex {
    /// Repo-relative directories; the root is the empty string.
    dirs: BTreeSet<String>,
}

impl SubprojectIndex {
    pub fn new<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let dirs = paths
            .into_iter()
            .filter_map(|path| {
                let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
                is_manifest(name).then(|| dir.to_string())
            })
            .collect();
        Self { dirs }
    }

    /// The nearest manifest directory above `path`, or [`ROOT_SUBPROJECT`].
    pub fn assign<'a>(&self, path: &'a str) -> &'a str {
        let mut dir = path;
        while let Some((parent, _)) = dir.rsplit_once('/') {
            if self.dirs.contains(parent) {
                return parent;
            }
            dir = parent;
        }
        ROOT_SUBPROJECT
    }
}

fn is_manifest(name: &str) -> bool {
    MANIFESTS.contains(&name)
        || MANIFEST_SUFFIXES
            .iter()
            .any(|suffix| name.len() > suffix.len() && name.ends_with(suffix))
}
