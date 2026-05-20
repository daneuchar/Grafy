//! Per-language tree-sitter `.scm` queries + lang-specific rules.
//! M1 W1: all 12 v1.0 languages shipped (plan §1).
//!
//! Each module exposes a `DEFINITIONS_SCM` const carrying the
//! tree-sitter query for structure pass (pass 1 + 2).

pub mod cpp;
pub mod csharp;
pub mod go;
pub mod java;
pub mod javascript;
pub mod lua;
pub mod php;
pub mod python;
pub mod rust;
pub mod scala;
pub mod tsx;
pub mod typescript;

use grafy_parser::Language;

/// Return the definitions `.scm` query for `lang`. Used by pass 1+2.
#[must_use]
pub fn definitions_scm(lang: Language) -> &'static str {
    match lang {
        Language::Rust => rust::DEFINITIONS_SCM,
        Language::Python => python::DEFINITIONS_SCM,
        Language::JavaScript => javascript::DEFINITIONS_SCM,
        Language::TypeScript => typescript::DEFINITIONS_SCM,
        Language::Tsx => tsx::DEFINITIONS_SCM,
        Language::Go => go::DEFINITIONS_SCM,
        Language::Java => java::DEFINITIONS_SCM,
        Language::Cpp => cpp::DEFINITIONS_SCM,
        Language::CSharp => csharp::DEFINITIONS_SCM,
        Language::Php => php::DEFINITIONS_SCM,
        Language::Scala => scala::DEFINITIONS_SCM,
        Language::Lua => lua::DEFINITIONS_SCM,
    }
}
