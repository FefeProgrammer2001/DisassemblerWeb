use std::env;
use std::fs;
use std::io::{self, Read};

use c_compiler_wasm::compile;

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut json_mode = false;
    let mut file_path = None;

    for arg in args.iter().skip(1) {
        if arg == "--json" {
            json_mode = true;
        } else if !arg.starts_with('-') {
            file_path = Some(arg.clone());
        }
    }

    let source = if let Some(path) = file_path {
        fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Error reading file '{}': {}", path, e);
            std::process::exit(1);
        })
    } else {
        // Check if stdin has data
        let mut buffer = String::new();
        if !atty_is_terminal() {
            io::stdin().read_to_string(&mut buffer).unwrap();
        }

        if buffer.trim().is_empty() {
            // Default demo code
            r#"
// Interactive C demo for visual disassembler
int factorial(int n) {
    if (n <= 1) {
        return 1;
    }
    return n * factorial(n - 1);
}

int main() {
    int x = 5;
    int *ptr = &x;
    int fact = factorial(*ptr);
    return fact;
}
"#
            .to_string()
        } else {
            buffer
        }
    };

    let result = compile(&source);

    if json_mode {
        let json = serde_json::to_string_pretty(&result).unwrap();
        println!("{}", json);
    } else {
        if !result.success {
            eprintln!("Compilation failed with errors:");
            for err in &result.errors {
                eprintln!("  [{}:{}] {}", err.line, err.column, err.message);
            }
            std::process::exit(1);
        }

        println!("=== GENERATED x86-64 ASSEMBLY ===");
        println!("{}", result.assembly);
        println!();

        println!("=== FUNCTION STACK FRAMES ===");
        for func in &result.functions {
            println!("Function '{}' (Stack Frame: {} bytes):", func.name, func.stack_frame_size);
            if !func.params.is_empty() {
                println!("  Parameters:");
                for p in &func.params {
                    println!("    - {} ({}, {} bytes) -> {} -> [rbp{}]", p.name, p.type_name, p.size, p.register_or_stack, p.rbp_offset);
                }
            }
            if !func.locals.is_empty() {
                println!("  Local Variables:");
                for l in &func.locals {
                    println!("    - {} ({}, {} bytes) -> [rbp{}]", l.name, l.type_name, l.size, l.rbp_offset);
                }
            }
            println!();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn atty_is_terminal() -> bool {
    unsafe { libc_isatty(0) != 0 }
}

#[cfg(not(target_arch = "wasm32"))]
extern "C" {
    #[link_name = "isatty"]
    fn libc_isatty(fd: i32) -> i32;
}

#[cfg(target_arch = "wasm32")]
fn atty_is_terminal() -> bool {
    false
}
