; Grafy M1 W3 — C# call sites.
; @call.name     — the method/function name being called
; @call.receiver — the object prefix for member access calls

; Simple invocation: foo(...)
(invocation_expression
  function: (identifier_name) @call.name)

; Member access invocation: obj.Method(...)
(invocation_expression
  function: (member_access_expression
    expression: (_) @call.receiver
    name: (identifier_name) @call.name))

; Object creation: new Foo(...)
(object_creation_expression
  type: (identifier_name) @call.name)
