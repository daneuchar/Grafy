; Grafy M1 W3 — C# using directives.
; @import.name   — alias (if any)
; @import.module — the namespace path

; using System.Collections;
(using_directive
  (qualified_name) @import.module)

; using System;
(using_directive
  (identifier) @import.module)

; using Alias = Some.Namespace;
(using_directive
  (name_equals
    (identifier) @import.name)
  (qualified_name) @import.module)
