pub mod ast;
pub mod codegen_x86_64;
pub mod lexer;
pub mod metadata;
pub mod parser;
pub mod stepper;
pub mod token;
pub mod wasm;

use codegen_x86_64::CodeGenerator;
use lexer::Lexer;
pub use metadata::{
    AsmInstructionKind, AsmLine, CompileError, CompileOutput, FunctionMeta, GlobalMeta,
    LocalVarMeta, ParamMeta, SourceSpan,
};
use parser::Parser;

/// Compiles C source code into x86-64 assembly with detailed metadata for visual disassembly
pub fn compile(source: &str) -> CompileOutput {
    let mut lexer = Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => return CompileOutput::error(e.message, e.line, e.column),
    };

    let mut parser = Parser::new(tokens);
    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => return CompileOutput::error(e.message, e.line, e.column),
    };

    let codegen = CodeGenerator::new();
    let (lines, functions, globals) = codegen.generate(&program);

    let assembly = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    CompileOutput {
        success: true,
        assembly,
        lines,
        functions,
        globals,
        errors: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_function() {
        let code = r#"
        int add(int a, int b) {
            return a + b;
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        assert_eq!(res.functions.len(), 1);
        let f = &res.functions[0];
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
        assert!(res.assembly.contains("add rax, rdi"));
    }

    #[test]
    fn test_local_variables_and_pointers() {
        let code = r#"
        int test() {
            int x = 42;
            int *p = &x;
            *p = 100;
            return x;
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        let f = &res.functions[0];
        assert_eq!(f.locals.len(), 2);
        assert_eq!(f.locals[0].name, "x");
        assert_eq!(f.locals[1].name, "p");
    }

    #[test]
    fn test_while_loop() {
        let code = r#"
        int sum_to(int n) {
            int total = 0;
            int i = 1;
            while (i <= n) {
                total += i;
                i++;
            }
            return total;
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        assert!(res.assembly.contains(".L.while_start"));
    }

    #[test]
    fn test_for_loop() {
        let code = r#"
        int factorial(int n) {
            int res = 1;
            for (int i = 1; i <= n; i++) {
                res *= i;
            }
            return res;
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        assert!(res.assembly.contains(".L.for_start"));
    }

    #[test]
    fn test_struct_declaration_and_member_access() {
        let code = r#"
        struct Point {
            int x;
            int y;
        };

        int test() {
            struct Point pt;
            pt.x = 10;
            pt.y = 20;
            return pt.x + pt.y;
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        assert_eq!(res.functions.len(), 1);
        let f = &res.functions[0];
        assert_eq!(f.locals.len(), 1);
        assert_eq!(f.locals[0].name, "pt");
        assert_eq!(f.locals[0].size, 8);
    }

    #[test]
    fn test_struct_pointer_arrow_access() {
        let code = r#"
        struct Node {
            int val;
            int next_offset;
        };

        int test() {
            struct Node n;
            struct Node *ptr = &n;
            ptr->val = 42;
            ptr->next_offset = 8;
            return ptr->val + ptr->next_offset;
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
    }

    #[test]
    fn test_multidimensional_arrays() {
        let code = r#"
        int test() {
            int matrix[2][3];
            matrix[0][0] = 1;
            matrix[0][1] = 2;
            matrix[1][2] = 42;
            return matrix[0][1] + matrix[1][2];
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        let f = &res.functions[0];
        assert_eq!(f.locals[0].name, "matrix");
        assert_eq!(f.locals[0].size, 24); // 2 * 3 * 4 = 24 bytes
    }

    #[test]
    fn test_pointer_dereference_assignment() {
        let code = r#"
        int test() {
            int arr[4];
            int *p = arr;
            *(p + 1) = 25;
            *(p + 3) = 75;
            return arr[1] + arr[3];
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
    }

    #[test]
    fn test_global_variables_read_write() {
        let code = r#"
        int g_counter = 100;
        int test() {
            g_counter += 42;
            return g_counter;
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        assert_eq!(res.globals.len(), 1);
        assert_eq!(res.globals[0].name, "g_counter");
        assert_eq!(res.globals[0].size, 4);
        assert!(res.globals[0].has_initializer);
        assert!(res.assembly.contains(".globl g_counter"));
        assert!(res.assembly.contains(".long 100"));
    }

    #[test]
    fn test_string_literals_and_data_section() {
        let code = r#"
        char *get_hello() {
            char *s = "hello";
            return s;
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        assert!(res.assembly.contains(".section .rodata"));
        assert!(res.assembly.contains(".L.str.0:"));
    }

    #[test]
    fn test_global_pointer_to_string() {
        let code = r#"
        char *g_msg = "hello world";
        int test() {
            return g_msg[0];
        }
        "#;
        let res = compile(code);
        assert!(res.success, "Compilation failed: {:?}", res.errors);
        assert_eq!(res.globals.len(), 1);
        assert_eq!(res.globals[0].name, "g_msg");
        assert!(res.assembly.contains(".quad .L.str.0"));
    }
}
