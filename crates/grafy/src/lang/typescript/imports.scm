; Grafy M1 W3 — TypeScript imports.
; @import.name   — the imported identifier
; @import.module — the source module specifier

; import { foo } from "./module"
(import_statement
  (import_clause
    (named_imports
      (import_specifier
        name: (identifier) @import.name)))
  source: (string) @import.module)

; import { foo as bar } from "./module"
(import_statement
  (import_clause
    (named_imports
      (import_specifier
        name: (identifier) @import.name
        alias: (identifier))))
  source: (string) @import.module)

; import defaultExport from "./module"
(import_statement
  (import_clause
    (identifier) @import.name)
  source: (string) @import.module)

; import * as ns from "./module"
(import_statement
  (import_clause
    (namespace_import
      (identifier) @import.name))
  source: (string) @import.module)

; require("./module")
(call_expression
  function: (identifier) @_require
  (#eq? @_require "require")
  arguments: (arguments (string) @import.module))
