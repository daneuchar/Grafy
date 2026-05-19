//! Cypher-Lite scope is fixed in plan §5. Unsupported features return
//! a structured error pointing the user to the structured-tool
//! replacement — they do not silently degrade.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    With,
    Unwind,
    Merge,
    Create,
    Delete,
    Set,
    Remove,
    VariableLengthPath,
    OptionalMatch,
    Aggregation,
    Function,
    MultipleMatch,
    PathVariable,
}

impl Unsupported {
    /// Error message template (plan §5).
    pub fn message(self) -> String {
        let (clause, replacement) = match self {
            Self::With => ("`WITH` clauses", "use the structured tool `search_graph`"),
            Self::Unwind => ("`UNWIND`", "iterate from the client side"),
            Self::Merge => ("`MERGE`", "Cypher-Lite is read-only"),
            Self::Create => ("`CREATE`", "Cypher-Lite is read-only"),
            Self::Delete => ("`DELETE`", "Cypher-Lite is read-only"),
            Self::Set => ("`SET`", "Cypher-Lite is read-only"),
            Self::Remove => ("`REMOVE`", "Cypher-Lite is read-only"),
            Self::VariableLengthPath => (
                "variable-length paths (`*`, `*1..3`)",
                "use the structured tool `trace_call_path` with bounded hops",
            ),
            Self::OptionalMatch => (
                "`OPTIONAL MATCH`",
                "use multiple `search_graph` calls",
            ),
            Self::Aggregation => (
                "aggregations (`count`, `sum`, `collect`, …)",
                "aggregate on the client side",
            ),
            Self::Function => (
                "functions (`toLower`, `coalesce`, …)",
                "filter on raw properties",
            ),
            Self::MultipleMatch => (
                "multiple `MATCH` clauses in one query",
                "issue separate queries",
            ),
            Self::PathVariable => (
                "path variables (`p = (a)-[*]->(b)`)",
                "use the structured tool `trace_call_path`",
            ),
        };
        format!(
            "ERROR: Cypher-Lite does not support {clause} (v1.0).\n       For this query, {replacement}. See docs/cypher-lite.md."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_shape() {
        let m = Unsupported::With.message();
        assert!(m.starts_with("ERROR: Cypher-Lite does not support"));
        assert!(m.contains("docs/cypher-lite.md"));
    }

    #[test]
    fn variable_length_routes_to_trace_call_path() {
        assert!(Unsupported::VariableLengthPath
            .message()
            .contains("trace_call_path"));
    }
}
