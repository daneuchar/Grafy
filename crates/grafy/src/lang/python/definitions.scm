; Grafy M0 — Python definitions. Minimal query, expanded in M1 week 1.
(function_definition name: (identifier) @function.name) @function.def
(class_definition    name: (identifier) @class.name) @class.def
(decorated_definition
  definition: (function_definition name: (identifier) @decorated.name)) @decorated.def
