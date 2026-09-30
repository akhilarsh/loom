use std::path::Path;

use anyhow::Result;

use crate::context::extract::dialect::{dialect_by_id, DialectSpec};
use crate::context::extract::{
    run_query, Capabilities, ExtractorIdentity, FileExtraction, QueryHarness, SourceGraphExtractor,
};
use crate::context::source_graph::{ImportBinding, NodeLanguage, SourceNodeKind, Span};

/// Extracts declarations, `#include`s and calls from `.c` files. A prototype is
/// not a definition: `@definition.function` sits on the `function_definition`,
/// so its span covers the body and calls inside it are attributed to the
/// function. A prototype is a `References` edge naming the function it
/// declares, which tells the resolver the project declares that name. Macros,
/// `#ifdef` selection and function-pointer targets are gaps: nothing is
/// expanded, so nothing is invented.
pub struct CExtractor;

impl CExtractor {
    pub fn new() -> Self {
        CExtractor
    }
}

impl Default for CExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// The patterns C and C++ share: type declarations (`typedef struct Foo {..}
/// Foo;` is one `type:Foo`, since the typedef's own type is named), includes,
/// prototypes, and calls by bare name or through a member. A macro so the C++
/// extractor can `concat!` its own patterns onto it without copying these.
macro_rules! c_family_query {
    () => {
        r#"
; A prototype (`int area(const Shape *s);`) declares a function without
; defining it: a `References` edge naming it, never a node.
(declaration
  declarator: [
    (function_declarator
      declarator: (identifier) @reference.name)
    (pointer_declarator
      declarator: (function_declarator
        declarator: (identifier) @reference.name))
  ])

(struct_specifier
  name: (type_identifier) @name
  body: (_)) @definition.type

(union_specifier
  name: (type_identifier) @name
  body: (_)) @definition.type

(enum_specifier
  name: (type_identifier) @name
  body: (_)) @definition.type

(type_definition
  type: (_ !name)
  declarator: (type_identifier) @name) @definition.type

(preproc_include
  path: [(string_literal) (system_lib_string)] @import.path) @import.statement

(call_expression
  function: (identifier) @call.name)

(call_expression
  function: (field_expression
    argument: (_) @call.receiver
    field: (field_identifier) @call.name))
"#
    };
}
pub(super) use c_family_query;

const QUERY: &str = concat!(
    r#"
(function_definition
  declarator: [
    (function_declarator
      declarator: (identifier) @name)
    (pointer_declarator
      declarator: (function_declarator
        declarator: (identifier) @name))
    (pointer_declarator
      declarator: (pointer_declarator
        declarator: (function_declarator
          declarator: (identifier) @name)))
  ]) @definition.function
"#,
    c_family_query!()
);

/// The one glob binding of an `#include`. `path` arrives with its quotes
/// stripped and its angle bracket kept, so `<stdio.h>` stays external and
/// `x.h` is resolved against the including file. The included file's names
/// arrive unqualified, so the binding is a glob and binds no local name.
pub(super) fn include_bindings(path: &str, site: Span) -> Vec<ImportBinding> {
    vec![ImportBinding {
        path: path.to_string(),
        name: None,
        alias: None,
        glob: true,
        site,
    }]
}

impl QueryHarness for CExtractor {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_c::LANGUAGE.into()
    }

    fn query_source(&self) -> &'static str {
        QUERY
    }

    fn identity(&self) -> ExtractorIdentity {
        ExtractorIdentity {
            dialect: "c",
            grammar_version: "0.24.2",
            query_digest: crate::context::source_graph::body_hash(QUERY.as_bytes()),
            extractor_version: 2,
        }
    }

    fn node_language(&self) -> NodeLanguage {
        NodeLanguage::C
    }

    fn kind_for_capture(&self, suffix: &str) -> Option<SourceNodeKind> {
        match suffix {
            "function" => Some(SourceNodeKind::Function),
            "type" => Some(SourceNodeKind::Type),
            _ => None,
        }
    }

    fn import_bindings(&self, _statement: &str, path: &str, site: Span) -> Vec<ImportBinding> {
        include_bindings(path, site)
    }
}

impl SourceGraphExtractor for CExtractor {
    fn dialect(&self) -> &'static DialectSpec {
        dialect_by_id("c").expect("the dialect table names every registered extractor")
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            declarations: true,
            imports: true,
            import_bindings: true,
            calls: true,
            receivers: true,
            references: false,
        }
    }

    fn cache_identity(&self) -> ExtractorIdentity {
        QueryHarness::identity(self)
    }

    fn extract(&self, path: &Path, bytes: &[u8]) -> Result<FileExtraction> {
        run_query(self, path, bytes)
    }
}

#[cfg(test)]
mod tests;
