# Project Progress & TDD Development Log

## Overview
**DisassemblerWeb** is an interactive web-based visual disassembler and memory execution simulator. Users write C code, and the tool compiles it into assembly while providing visual representations of the CPU registers, call stack frames, pointer movements, and memory state changes.

Currently, we have completed the core backend of **Phase 1: The C Compiler & CPU/Memory Stepper Engine (running inside WebAssembly)**.

---

## Architecture & Technology Choices
* **Implementation Language**: Rust
* **Target Output**: x86-64 Assembly (Intel syntax, System V AMD64 ABI compliant)
* **WASM Target**: `wasm32-unknown-unknown` (Standalone, zero external runtime dependencies, 289 KB release binary)
* **Metadata & Execution Capabilities**:
  * Line-by-line C source mapping (line and column spans)
  * Exact stack offsets (`[rbp - offset]`) and variable sizes
  * Function parameter register mapping (`EDI`, `ESI`, `RDX`, `RCX`, `R8D`, `R9D`)
  * Educational annotations per instruction (ALU, stack, memory read/write, branches)
  * Step-by-step CPU state machine with register diffs, memory access logs, and call stack visualization
  * Time-travel debugging support (`step_back()` snapshots)

---

## TDD (Test-Driven Development) Protocol
All code development follows strict TDD:
1. **Red**: Write a failing unit or integration test defining the expected behavior.
2. **Green**: Implement the minimal code required to pass the test.
3. **Refactor**: Clean up the implementation, optimize, ensure no regressions.
4. **WASM Verification**: Ensure `cargo test` passes natively and `node test_wasm.mjs` verifies the compiled `.wasm` output.
5. **Log**: Update this document with the new tests, features, and status.

---

## Implemented Features

### 1. Lexer & Tokenizer (`src/token.rs`, `src/lexer.rs`)
- [x] Keywords: `int`, `char`, `short`, `long`, `void`, `return`, `if`, `else`, `while`, `for`, `do`, `break`, `continue`, `sizeof`, `struct`
- [x] Numbers: Decimal and Hexadecimal (`0x...`) integer literals
- [x] Characters & Strings: Escape sequences (`\n`, `\t`, `\r`, `\0`, `\\`, `\'`, `\"`)
- [x] Comments: Single-line (`//`) and multi-line (`/* ... */`)
- [x] Operators: Arithmetic, bitwise, shift, comparison, logical, compound assignment, increment/decrement
- [x] Source span tracking for precise error diagnostics

### 2. Parser & AST (`src/ast.rs`, `src/parser.rs`)
- [x] Precedence climbing for binary and unary expressions
- [x] Pointer arithmetic scaling: `ptr + i` scales by `sizeof(*ptr)`
- [x] Multi-dimensional array indexing: `matrix[2][3]` nested dimension layout and lowering
- [x] Struct definition & layout calculation with standard alignment and field offsets
- [x] Member access expressions: Direct dot (`pt.x`) and pointer arrow (`ptr->x`)
- [x] Self-referential structs support (`struct Node { int val; struct Node *next; }`)
- [x] Global variable declarations with numbers and string literals
- [x] Control flow: `if`/`else`, `while`, `do-while`, `for`, `break`, `continue`, `return`
- [x] Local variable stack slot allocation with alignment (downwards from `RBP`)
- [x] AMD64 function parameter mapping

### 3. Code Generation (`src/codegen_x86_64.rs`)
- [x] Intel syntax x86-64 assembly generation
- [x] Prologue & epilogue (`push rbp; mov rbp, rsp; sub rsp, N; ...; mov rsp, rbp; pop rbp; ret`)
- [x] Saving register arguments to allocated local stack slots
- [x] Stack-based expression evaluation and ALU instructions
- [x] Struct member lvalue and rvalue code generation (`add rax, offset; mov eax, [rax]`)
- [x] Global variables in `.data` section and string literals in `.rodata` section
- [x] Short-circuit evaluation for `&&` and `||`
- [x] Rich metadata attached to every generated instruction line

### 4. CPU & Memory Stepper Engine (`src/stepper.rs`)
- [x] Virtual x86-64 CPU registers (`RAX`, `RBX`, `RCX`, `RDX`, `RSI`, `RDI`, `RBP`, `RSP`, `R8`, `R9`, etc.)
- [x] Accurate 32-bit register zero-extension rules (writes to `EAX` zero upper 32 bits of `RAX`)
- [x] Flags: `ZF` (Zero Flag) and `SF` (Sign Flag)
- [x] Simulated 64-bit sparse address space:
  - Stack grows downwards from `0x7FFF_FFFF_0000`
  - Globals at `0x1000_0000`
  - `.rodata` at `0x2000_0000`
- [x] Call stack frame inspection (`function_name`, `rbp`, `rsp`, `return_rip`)
- [x] Instruction execution: `push`, `pop`, `mov`, `movsx`, `lea`, `add`, `sub`, `imul`, `idiv`, `cqo`, `and`, `or`, `xor`, `shl`, `sar`, `neg`, `not`, `cmp`, `setcc`, `jmp`, `je`, `jne`, `call`, `ret`
- [x] Fine-grained event logging:
  - `RegisterChange` (register name, old value, new value)
  - `MemoryAccessEvent` (type: Read/Write, address, size in bytes, value, description)
- [x] Time-travel debugging: `step_back()` state snapshot restoration

### 5. WebAssembly Interface (`src/wasm.rs`, `test_wasm.mjs`)
- [x] Linear memory allocator (`wasm_alloc`, `wasm_free`)
- [x] Zero-copy C source ingestion and JSON response serialization (`compile_c`)
- [x] Stepper lifecycle FFI: `stepper_create`, `stepper_step`, `stepper_step_back`, `stepper_free`
- [x] Release build size: **289 KB**
- [x] Node.js test runner validating execution in WebAssembly:
  - Test 1: Recursion & Pointers (`factorial`)
  - Test 2: While loop with compound assignment
  - Test 3: Arrays (`int arr[3]`)
  - Test 4: Diagnostic error reporting with line/column
  - Test 5: Structs and Member Access (`struct Point`, `pt.x`, `ptr->x += 5`)
  - Test 6: Stepper Execution & Time-Travel in WASM (function calls, memory read/write events, register diffs, and stepping back)

---

## TDD Changelog

| Cycle | Feature | Red Test | Green Solution | Verification |
| :--- | :--- | :--- | :--- | :--- |
| **Cycle 1** | Struct declarations & member access (`.`, `->`) | `test_struct_declaration_and_member_access`, `test_struct_pointer_arrow_access` failed with `Expected type specifier, found Struct` | Added `Type::Struct`, `StructMember`, `Expr::MemberAccess`, struct layout calculator with field alignment padding, and codegen member address calculation. | `cargo test` 6/6 passed; `node test_wasm.mjs` (5 test suites) passed with 210 KB `.wasm`. |
| **Cycle 2** | Multi-dimensional arrays (`matrix[2][3]`) | `test_multidimensional_arrays` failed with `Expected ';', found LBracket` | Allowed multiple dimension brackets in `parse_declarator`, wrapping types innermost to outermost. | `cargo test` 8/8 passed. |
| **Cycle 3** | Global variables & string literals in data section | `test_global_pointer_to_string` failed with `Global variable initializers must currently be constant numbers, found StringLiteral` | Added `GlobalInit` enum (`Number`, `StringLiteral`, `None`), string literal registration for global pointers, and `.quad .L.str.N` codegen. | `cargo test` 11/11 passed. |
| **Cycle 4** | CPU & Memory Stepper Engine (Virtual Machine) | `test_stepper_basic_execution` failed with `Not yet implemented` | Built `Stepper` virtual machine with 64-bit registers, accurate x86-64 32-bit zero-extension semantics, sparse memory, call stack frames, `RegisterChange`, `MemoryAccessEvent`, time travel (`step_back`), and WASM FFI bindings. | `cargo test` 15/15 passed; `node test_wasm.mjs` (6 test suites) passed with 289 KB `.wasm`. |

---

## Roadmap & Next Phases

### Phase 2: Web Interface & Visual Disassembler UI
- [ ] Set up Vite + TypeScript + Tailwind (or preferred UI stack)
- [ ] Dual-pane synchronized code editor (C code ⟷ x86-64 Assembly)
- [ ] Interactive Call Stack visualization (dynamic frame boundary cards, `RBP`/`RSP` indicators)
- [ ] Registers panel (Hex/Dec view, animated pulse on register changes)
- [ ] Memory grid / Hex viewer with highlight animations on reads and writes
- [ ] Step Controls: Run, Pause, Step Forward, Step Back, Speed Slider, and Breakpoints
