; Grafy M1 W3 — Java call sites.
; @call.name     — the method/function name being called
; @call.receiver — the object before the dot

; Static or instance method call with object: obj.method(...)
(method_invocation
  object: (_) @call.receiver
  name: (identifier) @call.name)

; Unqualified method call: method(...)
(method_invocation
  name: (identifier) @call.name)

; Object creation: new Foo(...)
(object_creation_expression
  type: (type_identifier) @call.name)
