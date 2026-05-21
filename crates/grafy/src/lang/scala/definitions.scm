; Grafy M1 W1 — Scala definitions.
(function_definition name: (identifier) @function.name) @function.def
(class_definition name: (identifier) @class.name) @class.def
(trait_definition name: (identifier) @trait.name) @trait.def
(object_definition name: (identifier) @object.name) @object.def
; M1 W3: methods inside class/trait/object bodies.
; template_body is the shared body node for all three container kinds.
(template_body
  (function_definition name: (identifier) @method.name) @method.def)
