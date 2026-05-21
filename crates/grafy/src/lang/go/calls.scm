; Grafy M1 W3 — Go call sites.
; @call.name     — the function/method name being called
; @call.receiver — the selector prefix (for method calls)

; Plain function call: foo(...)
(call_expression
  function: (identifier) @call.name)

; Method / selector call: pkg.Func(...) or obj.Method(...)
(call_expression
  function: (selector_expression
    operand: (_) @call.receiver
    field: (field_identifier) @call.name))
