; Grafy M1 W3 — Lua require() calls (no first-class import statement).
; @import.name   — the variable receiving the module (if captured)
; @import.module — the module path string

; local foo = require("foo")
(local_variable_declaration
  (assignment_statement
    (variable_list
      name: (identifier) @import.name)
    (expression_list
      value: (function_call
        name: (identifier) @_req
        (#eq? @_req "require")
        arguments: (arguments (string) @import.module)))))

; foo = require("foo")
(variable_declaration
  (assignment_statement
    (variable_list
      name: (identifier) @import.name)
    (expression_list
      value: (function_call
        name: (identifier) @_req
        (#eq? @_req "require")
        arguments: (arguments (string) @import.module)))))

; require("foo") standalone
(function_call
  name: (identifier) @_req
  (#eq? @_req "require")
  arguments: (arguments (string) @import.module))
