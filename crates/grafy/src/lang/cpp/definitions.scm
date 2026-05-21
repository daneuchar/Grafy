; Grafy M1 W1 — C++ definitions.
(function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
(class_specifier name: (type_identifier) @class.name) @class.def
(struct_specifier name: (type_identifier) @struct.name) @struct.def
(enum_specifier name: (type_identifier) @enum.name) @enum.def
(namespace_definition name: (namespace_identifier) @ns.name) @ns.def
; M1 W3: inline methods defined inside a class body.
; field_declaration_list is the body node of class_specifier/struct_specifier.
; Inline methods use field_identifier (not identifier) as the leaf declarator.
(field_declaration_list
  (function_definition
    declarator: (function_declarator
      declarator: (field_identifier) @method.name)) @method.def)
