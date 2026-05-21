; Grafy M1 W3 — Rust imports (use declarations).
; @import.name   — the imported identifier (last segment or alias)
; @import.module — the path prefix

; use foo::bar;
(use_declaration
  argument: (scoped_identifier
    path: (_) @import.module
    name: (identifier) @import.name))

; use foo::bar::baz;  — multi-level, capture last segment
(use_declaration
  argument: (scoped_identifier
    name: (identifier) @import.name))

; use foo::bar as Alias;
(use_declaration
  argument: (scoped_use_list
    path: (_) @import.module))
