; Grafy M1 W3 — Rust call sites.
; Captures the callee name for plain calls foo() and method calls obj.method().
; @call.name  — the identifier being called (last segment)
; @call.receiver — the expression before the dot (for method calls)

; Plain function call: foo(...)
(call_expression
  function: (identifier) @call.name)

; Method call: expr.method(...)
(call_expression
  function: (field_expression
    value: (_) @call.receiver
    field: (field_identifier) @call.name))

; Path call: foo::bar(...) — capture the last segment
(call_expression
  function: (scoped_identifier
    name: (identifier) @call.name))
