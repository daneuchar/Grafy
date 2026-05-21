; Grafy M1 W3 — PHP call sites.
; @call.name     — the function/method name being called
; @call.receiver — the object prefix for method calls

; Plain function call: foo(...)
(function_call_expression
  function: (name) @call.name)

; Method call: $obj->method(...)
(member_call_expression
  object: (_) @call.receiver
  name: (name) @call.name)

; Static call: Foo::method(...)
(scoped_call_expression
  scope: (_) @call.receiver
  name: (name) @call.name)

; new Foo(...)
(object_creation_expression
  (named_type (name) @call.name))
