; Grafy M1 W3 — Lua call sites.
; @call.name     — the function/method name being called
; @call.receiver — the table prefix for method calls

; Plain call: foo(...)
(function_call
  name: (identifier) @call.name)

; Method call: tbl:method(...)  or  tbl.method(...)
(function_call
  name: (method_index_expression
    table: (_) @call.receiver
    method: (identifier) @call.name))

; Field call: tbl.func(...)
(function_call
  name: (dot_index_expression
    table: (_) @call.receiver
    field: (identifier) @call.name))
