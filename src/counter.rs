use syn::{visit::Visit, File as SynFile, Stmt};

/// A simple visitor that counts every `syn::Stmt` node in an AST.
pub struct StmtCounter {
    pub count: usize,
}

impl Default for StmtCounter {
    fn default() -> Self {
        Self::new()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_counter_with_zero_count() {
        let counter = StmtCounter::new();
        assert_eq!(
            counter.count, 0,
            "StmtCounter::new() should create a counter with count 0"
        );
    }

    #[test]
    fn test_default_creates_counter_with_zero_count() {
        let counter = StmtCounter::default();
        assert_eq!(
            counter.count, 0,
            "StmtCounter::default() should create a counter with count 0"
        );
    }

    #[test]
    fn test_counter_counts_simple_let_statements() {
        let code = r#"
            fn main() {
                let x = 5;
                let y = 10;
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(counter.count, 2, "Counter should count 2 let statements");
    }

    #[test]
    fn test_counter_counts_expression_statement() {
        let code = r#"
            fn main() {
                5 + 5;
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 1,
            "Counter should count 1 expression statement"
        );
    }

    #[test]
    fn test_counter_counts_multiple_statement_types() {
        let code = r#"
            fn main() {
                let x = 5;
                println!("Hello");
                if x > 0 {
                    let y = 10;
                }
                return;
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        // Note: The if block itself is a statement, and the let inside it is also counted
        assert_eq!(
            counter.count, 5,
            "Counter should count all statement types (let, expression, if with nested let, return)"
        );
    }

    #[test]
    fn test_counter_counts_nested_block_statements() {
        let code = r#"
            fn main() {
                let x = 5;
                if x > 0 {
                    let y = 10;
                    let z = 20;
                    if y > 5 {
                        let a = 30;
                    }
                }
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 6,
            "Counter should count statements in nested blocks"
        );
    }

    #[test]
    fn test_counter_counts_loop_statements() {
        let code = r#"
            fn main() {
                let mut count = 0;
                while count < 10 {
                    count += 1;
                }
                for i in 0..10 {
                    println!("{}", i);
                }
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 5,
            "Counter should count let, while, and for statements"
        );
    }

    #[test]
    fn test_counter_counts_match_statements() {
        let code = r#"
            fn main() {
                let x = 5;
                match x {
                    1 => println!("one"),
                    2 => println!("two"),
                    _ => println!("other"),
                }
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 2,
            "Counter should count let and match statements"
        );
    }

    #[test]
    fn test_counter_empty_file() {
        let code = r#"
            fn main() {
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 0,
            "Counter should return 0 for an empty function body"
        );
    }

    #[test]
    fn test_counter_multiple_functions() {
        let code = r#"
            fn foo() {
                let x = 1;
            }

            fn bar() {
                let y = 2;
                let z = 3;
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 3,
            "Counter should count statements across all functions"
        );
    }

    #[test]
    fn test_counter_struct_item_and_impl() {
        let code = r#"
            struct Foo {
                x: i32,
            }

            impl Foo {
                fn new(x: i32) -> Self {
                    Self { x }
                }

                fn get_x(&self) -> i32 {
                    self.x
                }
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 2,
            "Counter should count struct field initialization and return statements in impl blocks"
        );
    }

    #[test]
    fn test_counter_macro_invocation() {
        let code = r#"
            fn main() {
                println!("Hello");
                vec![1, 2, 3];
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 2,
            "Counter should count macro invocation statements"
        );
    }

    #[test]
    fn test_counter_async_function() {
        let code = r#"
            async fn foo() {
                let x = 5;
                x + 1
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        assert_eq!(
            counter.count, 2,
            "Counter should count statements in async functions"
        );
    }

    #[test]
    fn test_counter_unsafe_block() {
        let code = r#"
            fn main() {
                unsafe {
                    let x = 5;
                }
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        // Both the unsafe block statement and the let statement inside it are counted
        assert_eq!(
            counter.count, 2,
            "Counter should count unsafe block and statements inside it"
        );
    }

    #[test]
    fn test_counter_const_and_static() {
        let code = r#"
            const MAX: i32 = 100;

            fn main() {
                static COUNT: i32 = 0;
                let x = MAX;
            }
        "#;
        let ast: SynFile = syn::parse_file(code).expect("Failed to parse test code");
        let mut counter = StmtCounter::new();
        counter.visit_file(&ast);
        // static COUNT at item level is not a Stmt (it's an Item), but let x is a Stmt
        assert_eq!(
            counter.count, 2,
            "Counter should count let statement; static at item level is also counted as a statement by syn"
        );
    }
}
