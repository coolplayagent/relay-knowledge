; Keep definition ranges on the actual member, including bodyless protocol requirements.
(class_declaration name: (type_identifier) @name) @definition.class
(protocol_declaration name: (type_identifier) @name) @definition.interface
(function_declaration name: (simple_identifier) @name) @definition.function
(protocol_function_declaration name: (simple_identifier) @name) @definition.method
(init_declaration "init" @name) @definition.constructor
(deinit_declaration "deinit" @name) @definition.method
(subscript_declaration "subscript" @name) @definition.method
(property_declaration (pattern (simple_identifier) @name)) @definition.property
