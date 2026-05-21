; Grafy M1 W3 — Scala call sites.
; @call.name     — the function/method name
; @call.receiver — the prefix object for method calls

; Simple call: foo(...)
(call_expression
  function: (identifier) @call.name)

; Field access call: obj.method(...)
(call_expression
  function: (field_expression
    value: (_) @call.receiver
    field: (identifier) @call.name))
