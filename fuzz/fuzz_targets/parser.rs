//! Parser fuzz target — plan §M0 acceptance.
//!
//! Run with `cargo fuzz run parser`. Required: ≥60 min without panic
//! before M0 gate. Wraps both Rust and Python grammars.

#![no_main]

use grafy_parser::{parse, Language};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse("fuzz.rs", Language::Rust, data);
    let _ = parse("fuzz.py", Language::Python, data);
});
