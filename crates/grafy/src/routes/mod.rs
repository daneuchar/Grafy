//! HTTP route framework registry — M1 W4 (plan §4).
//!
//! Per-framework tree-sitter query files co-located under sub-modules.
//! Detection is lexical (byte-substring scan), cheap, and per-file —
//! multi-framework repos are common.

pub mod express;
pub mod fastapi;
pub mod gin;

use std::path::Path;

use grafy_parser::Language;

/// Supported HTTP frameworks for v1.0 route extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    /// FastAPI (Python) — decorator pattern `@app.get("/path")`.
    FastApi,
    /// Gin (Go) — call-expression pattern `r.GET("/path", handler)`.
    Gin,
    /// Express (Node/TS) — call-expression pattern `app.get("/path", handler)`.
    Express,
}

/// Return the `.scm` query for `framework`.
#[must_use]
pub fn routes_scm(fw: Framework) -> &'static str {
    match fw {
        Framework::FastApi => fastapi::ROUTES_SCM,
        Framework::Gin => gin::ROUTES_SCM,
        Framework::Express => express::ROUTES_SCM,
    }
}

/// Detect which HTTP framework (if any) a single file belongs to.
///
/// Detection is cheap and lexical — scan `source` for well-known byte substrings.
/// Returns `None` if no supported framework is detected or if the language does
/// not match any supported framework.
#[must_use]
pub fn detect_framework(lang: Language, _file: &Path, source: &[u8]) -> Option<Framework> {
    match lang {
        Language::Python => {
            if contains(source, b"from fastapi import") || contains(source, b"import fastapi") {
                return Some(Framework::FastApi);
            }
            None
        }
        Language::Go => {
            if contains(source, b"gin.New(")
                || contains(source, b"*gin.Engine")
                || contains(source, b"gin.Default()")
                || contains(source, b"\"github.com/gin-gonic/gin\"")
            {
                return Some(Framework::Gin);
            }
            None
        }
        Language::JavaScript | Language::TypeScript | Language::Tsx => {
            if contains(source, b"require('express')")
                || contains(source, b"require(\"express\")")
                || contains(source, b"from 'express'")
                || contains(source, b"from \"express\"")
            {
                return Some(Framework::Express);
            }
            None
        }
        // Other languages have no supported framework in v1.0.
        _ => None,
    }
}

/// Byte-substring scan — avoids allocating a `String` for matching.
#[inline]
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grafy_parser::Language;

    #[test]
    fn detect_fastapi() {
        let src = b"from fastapi import FastAPI\napp = FastAPI()";
        assert_eq!(
            detect_framework(Language::Python, Path::new("app.py"), src),
            Some(Framework::FastApi)
        );
    }

    #[test]
    fn detect_gin() {
        let src = b"import \"github.com/gin-gonic/gin\"\nfunc main() { r := gin.Default() }";
        assert_eq!(
            detect_framework(Language::Go, Path::new("main.go"), src),
            Some(Framework::Gin)
        );
    }

    #[test]
    fn detect_express_require() {
        let src = b"const express = require('express');\nconst app = express();";
        assert_eq!(
            detect_framework(Language::JavaScript, Path::new("app.js"), src),
            Some(Framework::Express)
        );
    }

    #[test]
    fn detect_express_esm() {
        let src = b"import express from 'express';\nconst app = express();";
        assert_eq!(
            detect_framework(Language::TypeScript, Path::new("app.ts"), src),
            Some(Framework::Express)
        );
    }

    #[test]
    fn detect_none_for_rust() {
        let src = b"fn main() {}";
        assert_eq!(
            detect_framework(Language::Rust, Path::new("main.rs"), src),
            None
        );
    }

    #[test]
    fn detect_none_for_plain_python() {
        let src = b"import os\ndef foo(): pass";
        assert_eq!(
            detect_framework(Language::Python, Path::new("util.py"), src),
            None
        );
    }
}
