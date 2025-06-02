use syn::{visit::Visit, File as SynFile, Stmt};

/// A simple visitor that counts every `syn::Stmt` node in an AST.
pub struct StmtCounter {
    pub count: usize,
}

impl StmtCounter {
    /// Create a new, empty `StmtCounter`.
    pub fn new() -> Self {
        StmtCounter { count: 0 }
    }
}

impl<'ast> Visit<'ast> for StmtCounter {
    /// Called for each `Stmt` in the AST.
    /// We increment `count` and continue walking nested statements.
    fn visit_stmt(&mut self, node: &'ast Stmt) {
        self.count += 1;
        syn::visit::visit_stmt(self, node);
    }

    // We need to implement visit_file to allow StmtCounter to be used with visit_file(&ast)
    fn visit_file(&mut self, node: &'ast SynFile) {
        syn::visit::visit_file(self, node);
    }
}
