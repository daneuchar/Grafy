; Grafy M1 W4 — Express route captures.
;
; Matches the Express call-expression pattern:
;   app.get('/path', handler)
;   app.post('/path', handler)
;   etc.
;
; Two variants:
;   1. Named handler: second argument is an identifier → @route.handler is the name.
;   2. Inline arrow/function: second argument is an arrow_function or function_expression
;      → @route.handler is NOT captured here; pass4 detects the inline case by
;      absence of @route.handler and synthesises a name from file:line.
;
; Captures:
;   @route.method   — the HTTP verb property name (get, post, put, delete, patch, etc.)
;   @route.pattern  — the string literal path (single or double quotes)
;   @route.handler  — the handler identifier (only for named handlers)

; Named handler variant.
(call_expression
  function: (member_expression
    property: (property_identifier) @route.method)
  arguments: (arguments
    (string) @route.pattern
    (identifier) @route.handler))

; Inline arrow function variant — captures route.method and route.pattern only.
; Pass 4 synthesises a handler name from file:line of the arrow_function node.
(call_expression
  function: (member_expression
    property: (property_identifier) @route.method)
  arguments: (arguments
    (string) @route.pattern
    (arrow_function) @route.inline_handler))

; Inline function expression variant.
(call_expression
  function: (member_expression
    property: (property_identifier) @route.method)
  arguments: (arguments
    (string) @route.pattern
    (function_expression) @route.inline_handler))
