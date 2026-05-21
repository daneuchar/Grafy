; Grafy M1 W3 — JavaScript call sites.
; @call.name     — the function/method name being called
; @call.receiver — the object prefix for method calls

; Plain call: foo(...)
(call_expression
  function: (identifier) @call.name)

; Method call: obj.method(...)
(call_expression
  function: (member_expression
    object: (_) @call.receiver
    property: (property_identifier) @call.name))

; new Foo(...)
(new_expression
  constructor: (identifier) @call.name)

; new foo.Bar(...)
(new_expression
  constructor: (member_expression
    object: (_) @call.receiver
    property: (property_identifier) @call.name))
