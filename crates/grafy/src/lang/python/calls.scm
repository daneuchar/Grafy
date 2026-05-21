; Grafy M1 W3 — Python call sites.
; @call.name     — the callable name (identifier or attribute)
; @call.receiver — the object before the dot (for attribute calls)

; Plain call: foo(...)
(call
  function: (identifier) @call.name)

; Attribute call: obj.method(...)
(call
  function: (attribute
    object: (_) @call.receiver
    attribute: (identifier) @call.name))
