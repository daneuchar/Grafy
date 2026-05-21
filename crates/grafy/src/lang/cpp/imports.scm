; Grafy M1 W3 — C++ includes and using declarations.
; @import.name   — the used name (for using declarations)
; @import.module — the included file path

; #include <header>
(preproc_include
  path: (system_lib_string) @import.module)

; #include "header"
(preproc_include
  path: (string_literal) @import.module)

; using namespace Foo;
(using_declaration
  (scoped_identifier) @import.module)

; using Foo::Bar;
(using_declaration
  (scoped_identifier
    name: (identifier) @import.name))
