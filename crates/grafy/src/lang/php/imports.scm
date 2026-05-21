; Grafy M1 W3 — PHP use declarations.
; @import.name   — the simple name (last segment or alias)
; @import.module — the full namespace path

; use Foo\Bar\Baz;
(namespace_use_declaration
  (namespace_use_clause
    (qualified_name) @import.module))

; use Foo\Bar\Baz as Alias;
(namespace_use_declaration
  (namespace_use_clause
    (qualified_name) @import.module
    (namespace_aliasing_clause
      (name) @import.name)))
