; Grafy M1 W1 — JavaScript definitions.
(function_declaration name: (identifier) @function.name) @function.def
(class_declaration    name: (identifier) @class.name) @class.def
(method_definition    name: (property_identifier) @method.name) @method.def
(lexical_declaration
  (variable_declarator
    name: (identifier) @arrow.name
    value: (arrow_function))) @arrow.def
