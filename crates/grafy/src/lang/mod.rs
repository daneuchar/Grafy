//! Per-language tree-sitter `.scm` queries + lang-specific rules.
//! M1 W1: all 12 v1.0 languages shipped (plan §1).
//! M1 W3: calls.scm + imports.scm dispatchers added.
//!
//! Each module exposes:
//!   `DEFINITIONS_SCM` — structure pass (pass 1 + 2)
//!   `CALLS_SCM`       — call-site capture pass (pass 3)
//!   `IMPORTS_SCM`     — import/use capture pass (pass 3)

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

/// Return the calls `.scm` query for `lang`. Used by pass 3 to find call sites.
#[must_use]
pub fn calls_scm(lang: Language) -> &'static str {
    match lang {
        Language::Rust => rust::CALLS_SCM,
        Language::Python => python::CALLS_SCM,
        Language::JavaScript => javascript::CALLS_SCM,
        Language::TypeScript => typescript::CALLS_SCM,
        Language::Tsx => tsx::CALLS_SCM,
        Language::Go => go::CALLS_SCM,
        Language::Java => java::CALLS_SCM,
        Language::Cpp => cpp::CALLS_SCM,
        Language::CSharp => csharp::CALLS_SCM,
        Language::Php => php::CALLS_SCM,
        Language::Scala => scala::CALLS_SCM,
        Language::Lua => lua::CALLS_SCM,
    }
}

/// Return the imports `.scm` query for `lang`. Used by pass 3 to find import bindings.
#[must_use]
pub fn imports_scm(lang: Language) -> &'static str {
    match lang {
        Language::Rust => rust::IMPORTS_SCM,
        Language::Python => python::IMPORTS_SCM,
        Language::JavaScript => javascript::IMPORTS_SCM,
        Language::TypeScript => typescript::IMPORTS_SCM,
        Language::Tsx => tsx::IMPORTS_SCM,
        Language::Go => go::IMPORTS_SCM,
        Language::Java => java::IMPORTS_SCM,
        Language::Cpp => cpp::IMPORTS_SCM,
        Language::CSharp => csharp::IMPORTS_SCM,
        Language::Php => php::IMPORTS_SCM,
        Language::Scala => scala::IMPORTS_SCM,
        Language::Lua => lua::IMPORTS_SCM,
    }
}
