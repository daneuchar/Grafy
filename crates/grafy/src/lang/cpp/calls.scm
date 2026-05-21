; Grafy M1 W3 — C++ call sites.
; @call.name     — the function/method name being called
; @call.receiver — the object/class prefix for member calls

; Plain function call: foo(...)
(call_expression
  function: (identifier) @call.name)

; Field access call: obj.method(...)
(call_expression
  function: (field_expression
    argument: (_) @call.receiver
    field: (field_identifier) @call.name))

; Pointer access call: ptr->method(...)
(call_expression
  function: (field_expression
    argument: (_) @call.receiver
    field: (field_identifier) @call.name))

; Scoped call: Foo::bar(...)
(call_expression
  function: (scoped_identifier
    name: (identifier) @call.name))
