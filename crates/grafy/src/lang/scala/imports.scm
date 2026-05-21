; Grafy M1 W3 — Scala imports.
; @import.name   — the imported identifier
; @import.module — the package/object path

; import foo.bar.Baz
(import_declaration
  (stable_identifier) @import.module)

; import foo.bar.{Baz, Qux}
(import_declaration
  (import_selectors
    (import_selector
      (identifier) @import.name)))
