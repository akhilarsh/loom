//! Import bindings: the names a file's import statements bring into scope.

use serde::{Deserialize, Serialize};

use super::Span;

/// One name (or whole module) an import statement binds in its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportBinding {
    /// Module spec as written, quotes stripped. Lossless: two statements that
    /// resolve differently never share a spec.
    pub path: String,
    /// Imported member (`parse` in `import { parse as p }`); `None` means the
    /// whole module or namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Local alias (`p`). `None` binds under `name`, or the module's last
    /// segment. Extractors set it explicitly whenever the bound local name is
    /// not the last path segment (Python `import a.b` gives `Some("a.b")`), and
    /// to `Some("")` for a side-effect import that binds nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// `use x::*`, `from x import *`, `import a.b.*`, `using A.B;`, `#include`,
    /// `require`.
    #[serde(default)]
    pub glob: bool,
    /// Where the statement sits in the file.
    pub site: Span,
}

impl ImportBinding {
    /// The local name this import binds: `alias`, else `name`, else the last
    /// path segment. `None` for a glob and for a side-effect import
    /// (`alias == Some("")`).
    pub fn local_name(&self) -> Option<&str> {
        if self.glob || self.alias.as_deref() == Some("") {
            return None;
        }
        self.alias
            .as_deref()
            .or(self.name.as_deref())
            .or_else(|| last_segment(&self.path))
    }
}

/// The last segment of a module path, split on the separators the supported
/// languages use (`::`, `.`, `/`, `\`).
fn last_segment(path: &str) -> Option<&str> {
    path.rsplit(['/', '.', ':', '\\'])
        .find(|segment| !segment.is_empty())
}
