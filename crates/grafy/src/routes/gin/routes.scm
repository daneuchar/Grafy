; Grafy M1 W4 — Gin route captures.
;
; Matches the Gin call-expression pattern:
;   r.GET("/path", handler)
;   r.POST("/path", handler)
;   etc.
;
; The receiver can be any identifier (r, router, engine, etc.).
; The method (GET, POST, etc.) is the field of the selector_expression.
; The first argument is the path string literal.
; The second argument is the handler identifier.
;
; Captures:
;   @route.method   — the HTTP verb (GET, POST, PUT, DELETE, PATCH, OPTIONS, HEAD)
;   @route.pattern  — the string literal path
;   @route.handler  — the handler function identifier

(call_expression
  function: (selector_expression
    field: (field_identifier) @route.method)
  arguments: (argument_list
    (interpreted_string_literal) @route.pattern
    (identifier) @route.handler))
