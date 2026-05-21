; Grafy M1 W4 — FastAPI route captures.
;
; Matches the FastAPI decorator pattern:
;   @app.get("/path")
;   @router.post("/path")
;   etc.
;
; The decorator is an `attribute` call where the attribute name is the HTTP verb.
; The function immediately following the decorator(s) is the handler.
;
; Captures:
;   @route.method   — the HTTP verb identifier (get, post, put, delete, patch, options, head)
;   @route.pattern  — the string literal path argument
;   @route.handler  — the function name defined after the decorator

(decorated_definition
  (decorator
    (call
      function: (attribute
        attribute: (identifier) @route.method)
      arguments: (argument_list
        (string) @route.pattern)))
  definition: (function_definition
    name: (identifier) @route.handler))
