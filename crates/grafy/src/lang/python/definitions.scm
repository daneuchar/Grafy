; Grafy M0 — Python definitions. Minimal query, expanded in M1 week 1.
(function_definition name: (identifier) @function.name) @function.def
(class_definition    name: (identifier) @class.name) @class.def
(decorated_definition
  definition: (function_definition name: (identifier) @decorated.name)) @decorated.def
; M1 W3: methods inside class bodies.
; Anchors to class body block so top-level functions are not double-counted.
(class_definition
  body: (block
    (function_definition name: (identifier) @method.name) @method.def))
