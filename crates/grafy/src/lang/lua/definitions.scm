; Grafy M1 W1 — Lua definitions.
(function_declaration name: (identifier) @function.name) @function.def
(function_declaration name: (dot_index_expression
  table: (identifier) @method.table
  field: (identifier) @method.name)) @method.def
(variable_declaration
  (assignment_statement
    (variable_list name: (identifier) @local.name)
    (expression_list value: (function_definition)))) @local.def
