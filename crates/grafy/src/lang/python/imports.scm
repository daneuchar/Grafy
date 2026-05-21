; Grafy M1 W3 — Python imports.
; @import.name   — the imported identifier (as it appears in the namespace)
; @import.module — the source module path

; import foo
(import_statement
  name: (dotted_name) @import.name)

; import foo.bar — capture the module
(import_statement
  name: (dotted_name
    (identifier) @import.module))

; from foo import bar
(import_from_statement
  module_name: (dotted_name) @import.module
  name: (dotted_name) @import.name)

; from foo import bar as baz
(import_from_statement
  module_name: (dotted_name) @import.module
  name: (aliased_import
    name: (dotted_name) @import.name))

; from foo import (bar, baz)
(import_from_statement
  module_name: (dotted_name) @import.module
  name: (wildcard_import))
