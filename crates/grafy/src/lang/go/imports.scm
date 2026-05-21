; Grafy M1 W3 — Go imports.
; @import.name   — the package alias (or last path segment if no alias)
; @import.module — the import path string

; import "fmt"
(import_spec
  path: (interpreted_string_literal) @import.module)

; import fmt "fmt"  (aliased)
(import_spec
  name: (package_identifier) @import.name
  path: (interpreted_string_literal) @import.module)
