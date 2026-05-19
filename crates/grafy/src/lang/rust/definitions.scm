; Grafy M0 — Rust definitions. Minimal query, expanded in M1 week 1.
(function_item name: (identifier) @function.name) @function.def
(struct_item   name: (type_identifier) @struct.name) @struct.def
(enum_item     name: (type_identifier) @enum.name) @enum.def
(trait_item    name: (type_identifier) @trait.name) @trait.def
(impl_item     trait: (type_identifier)? @impl.trait
               type: (type_identifier) @impl.type) @impl.def
(mod_item      name: (identifier) @mod.name) @mod.def
