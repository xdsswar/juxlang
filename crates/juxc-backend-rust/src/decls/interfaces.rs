//! Jux interface declarations → Rust `trait`.

use juxc_ast::ReturnType;

use crate::RustEmitter;

impl RustEmitter {
    /// Lower a Jux interface to a Rust `trait`. Method signatures
    /// emit directly — `void foo();` becomes `fn foo(&self);` —
    /// and Turn-1 interfaces have no default-method bodies.
    ///
    /// **Receiver kind.** Trait methods always use `&self` in Turn 1.
    /// If a class implementing the interface needs to mutate state in
    /// its method body, the user has to mark that method non-interface
    /// — the cross-class receiver-kind analysis isn't in yet. See the
    /// Turn-1 limitations note in the interface doc.
    pub(crate) fn emit_interface_decl(&mut self, interface: &juxc_ast::InterfaceDecl) {
        // (Migrated to Writer indent-aware API)
        self.w.emit_indent();
        self.emit_visibility(interface.visibility);
        self.w.push_str("trait ");
        self.w.push_str(&interface.name.text);
        // Generic params follow without bounds — the trait doesn't
        // imply `Clone` itself; implementing types pick up bounds as
        // needed on their own impls.
        self.emit_generic_params(&interface.generic_params);
        self.w.push_str(" {\n");
        self.w.indent_inc();
        for method in &interface.methods {
            self.w.emit_indent();
            self.w.push_str("fn ");
            self.w.push_str(&method.name.text);
            self.emit_generic_params(&method.generic_params);
            self.w.push_str("(&self");
            for param in &method.params {
                self.w.push_str(", ");
                self.w.push_str(&param.name.text);
                self.w.push_str(": ");
                self.emit_type_as_rust(&param.ty);
            }
            self.w.push(')');
            match &method.return_type {
                ReturnType::Void => {}
                ReturnType::Type(t) => {
                    self.w.push_str(" -> ");
                    self.emit_return_type_as_rust(t);
                }
                ReturnType::AsyncType(_) => {
                    self.w.push_str(" -> ()");
                }
            }
            self.w.push_str(";\n");
        }
        self.w.indent_dec();
        self.w.line("}");
        self.w.newline();
    }
}
