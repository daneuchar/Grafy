//! Fully-qualified-name rules per language. Plan §4 M1 W1 ships all 12.

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

use std::path::Path;

use grafy_parser::Language;

/// Dispatch a file path to its language's FQN rule.
/// Returns `None` if the language has no path-derivable FQN
/// (e.g. dynamic languages where the FQN depends on imports).
#[must_use]
pub fn for_file(lang: Language, root: &Path, file: &Path) -> Option<String> {
    match lang {
        Language::Rust => rust::fqn(root, file),
        Language::Python => python::fqn(root, file),
        Language::JavaScript => javascript::fqn(root, file),
        Language::TypeScript => typescript::fqn(root, file),
        Language::Tsx => tsx::fqn(root, file),
        Language::Go => go::fqn(root, file),
        Language::Java => java::fqn(root, file),
        Language::Cpp => cpp::fqn(root, file),
        Language::CSharp => csharp::fqn(root, file),
        Language::Php => php::fqn(root, file),
        Language::Scala => scala::fqn(root, file),
        Language::Lua => lua::fqn(root, file),
    }
}
