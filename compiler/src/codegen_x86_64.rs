use crate::ast::*;
use crate::metadata::*;

pub struct CodeGenerator {
    lines: Vec<AsmLine>,
    label_counter: usize,
    current_func: Option<String>,
    current_return_label: Option<String>,
    current_break_label: Option<String>,
    current_continue_label: Option<String>,
}

impl CodeGenerator {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            label_counter: 0,
            current_func: None,
            current_return_label: None,
            current_break_label: None,
            current_continue_label: None,
        }
    }

    fn new_label(&mut self, prefix: &str) -> String {
        self.label_counter += 1;
        format!(".L.{}.{}", prefix, self.label_counter)
    }

    fn emit(
        &mut self,
        text: &str,
        kind: AsmInstructionKind,
        comment: Option<&str>,
        span: Option<SourceSpan>,
    ) {
        let line_number = self.lines.len() + 1;
        self.lines.push(AsmLine {
            line_number,
            text: text.to_string(),
            source_line: span.map(|s| s.start_line),
            source_span: span,
            comment: comment.map(|c| c.to_string()),
            kind,
        });
    }

    fn emit_directive(&mut self, text: &str) {
        self.emit(text, AsmInstructionKind::Directive, None, None);
    }

    fn emit_label(&mut self, label: &str, comment: Option<&str>) {
        self.emit(
            &format!("{}:", label),
            AsmInstructionKind::Label,
            comment,
            None,
        );
    }

    fn emit_inst(
        &mut self,
        inst: &str,
        kind: AsmInstructionKind,
        comment: Option<&str>,
        span: Option<SourceSpan>,
    ) {
        let padded = format!("    {}", inst);
        self.emit(&padded, kind, comment, span);
    }

    pub fn generate(mut self, program: &Program) -> (Vec<AsmLine>, Vec<FunctionMeta>, Vec<GlobalMeta>) {
        self.emit_directive(".intel_syntax noprefix");

        // String literals in .rodata
        if !program.string_literals.is_empty() {
            self.emit_directive(".section .rodata");
            for (id, bytes) in &program.string_literals {
                let lbl = format!(".L.str.{}", id);
                self.emit_label(&lbl, Some("String literal"));
                let mut byte_strs = Vec::new();
                for b in bytes {
                    byte_strs.push(b.to_string());
                }
                self.emit_directive(&format!("    .byte {}", byte_strs.join(", ")));
            }
        }

        // Global variables
        let mut globals_meta = Vec::new();
        if !program.globals.is_empty() {
            self.emit_directive(".data");
            for global in &program.globals {
                globals_meta.push(GlobalMeta {
                    name: global.name.clone(),
                    type_name: global.ty.type_name(),
                    size: global.ty.size(),
                    has_initializer: !matches!(global.init_val, GlobalInit::None),
                });

                self.emit_directive(&format!(".globl {}", global.name));
                self.emit_label(&global.name, Some(&format!("Global variable '{}'", global.name)));

                match global.init_val {
                    GlobalInit::Number(val) => {
                        match global.ty.size() {
                            1 => self.emit_directive(&format!("    .byte {}", val as u8)),
                            2 => self.emit_directive(&format!("    .value {}", val as u16)),
                            4 => self.emit_directive(&format!("    .long {}", val as u32)),
                            8 => self.emit_directive(&format!("    .quad {}", val)),
                            _ => self.emit_directive(&format!("    .zero {}", global.ty.size())),
                        }
                    }
                    GlobalInit::StringLiteral(id) => {
                        self.emit_directive(&format!("    .quad .L.str.{}", id));
                    }
                    GlobalInit::None => {
                        self.emit_directive(&format!("    .zero {}", global.ty.size()));
                    }
                }
            }
        }

        // Functions in .text
        self.emit_directive(".text");
        let mut functions_meta = Vec::new();

        for func in &program.functions {
            let start_line = self.lines.len() + 1;
            let meta = self.gen_function(func);
            let end_line = self.lines.len();

            let mut f_meta = meta;
            f_meta.asm_start_line = start_line;
            f_meta.asm_end_line = end_line;
            functions_meta.push(f_meta);
        }

        (self.lines, functions_meta, globals_meta)
    }

    fn gen_function(&mut self, func: &FunctionDef) -> FunctionMeta {
        let return_label = format!(".L.return.{}", func.name);
        self.current_func = Some(func.name.clone());
        self.current_return_label = Some(return_label.clone());

        // Function symbol and label
        self.emit_directive(&format!(".globl {}", func.name));
        self.emit_directive(&format!(".type {}, @function", func.name));
        self.emit_label(&func.name, Some(&format!("Function: {}", func.name)));

        // Prologue
        let span = Some(func.span);
        self.emit_inst(
            "push rbp",
            AsmInstructionKind::Prologue,
            Some("Save old base pointer"),
            span,
        );
        self.emit_inst(
            "mov rbp, rsp",
            AsmInstructionKind::Prologue,
            Some("Establish new stack frame base"),
            span,
        );

        if func.stack_size > 0 {
            self.emit_inst(
                &format!("sub rsp, {}", func.stack_size),
                AsmInstructionKind::Prologue,
                Some(&format!("Allocate {} bytes on stack for locals", func.stack_size)),
                span,
            );
        }

        // Save incoming argument registers to their assigned stack slots
        let arg_regs_64 = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        let arg_regs_32 = ["edi", "esi", "edx", "ecx", "r8d", "r9d"];
        let arg_regs_16 = ["di", "si", "dx", "cx", "r8w", "r9w"];
        let arg_regs_8 = ["dil", "sil", "dl", "cl", "r8b", "r9b"];

        let mut params_meta = Vec::new();

        for (i, (param_name, param_ty)) in func.params.iter().enumerate() {
            if let Some(local) = func.locals.iter().find(|l| &l.name == param_name) {
                if i < 6 {
                    let reg = match local.ty.size() {
                        1 => arg_regs_8[i],
                        2 => arg_regs_16[i],
                        4 => arg_regs_32[i],
                        _ => arg_regs_64[i],
                    };
                    let ptr_specifier = match local.ty.size() {
                        1 => "BYTE PTR",
                        2 => "WORD PTR",
                        4 => "DWORD PTR",
                        _ => "QWORD PTR",
                    };
                    let offset_str = if local.rbp_offset < 0 {
                        format!("rbp{}", local.rbp_offset)
                    } else {
                        format!("rbp+{}", local.rbp_offset)
                    };

                    self.emit_inst(
                        &format!("mov {} [{}], {}", ptr_specifier, offset_str, reg),
                        AsmInstructionKind::MemoryWrite,
                        Some(&format!("Save parameter '{}' to stack slot", param_name)),
                        span,
                    );

                    params_meta.push(ParamMeta {
                        name: param_name.clone(),
                        type_name: param_ty.type_name(),
                        size: local.size,
                        register_or_stack: reg.to_uppercase(),
                        rbp_offset: local.rbp_offset,
                    });
                }
            }
        }

        // Generate function body
        for stmt in &func.body {
            self.gen_stmt(stmt, func);
        }

        // Epilogue
        self.emit_label(&return_label, Some("Function return target"));
        self.emit_inst(
            "mov rsp, rbp",
            AsmInstructionKind::Epilogue,
            Some("Deallocate local stack frame"),
            span,
        );
        self.emit_inst(
            "pop rbp",
            AsmInstructionKind::Epilogue,
            Some("Restore caller base pointer"),
            span,
        );
        self.emit_inst(
            "ret",
            AsmInstructionKind::Return,
            Some("Return to caller"),
            span,
        );

        let locals_meta: Vec<LocalVarMeta> = func
            .locals
            .iter()
            .map(|l| LocalVarMeta {
                name: l.name.clone(),
                type_name: l.ty.type_name(),
                size: l.size,
                rbp_offset: l.rbp_offset,
            })
            .collect();

        FunctionMeta {
            name: func.name.clone(),
            return_type: func.ret_type.type_name(),
            stack_frame_size: func.stack_size,
            params: params_meta,
            locals: locals_meta,
            asm_start_line: 0,
            asm_end_line: 0,
        }
    }

    fn gen_stmt(&mut self, stmt: &Stmt, func: &FunctionDef) {
        let span = Some(stmt.span());

        match stmt {
            Stmt::Expr(expr) => {
                self.gen_expr(expr, func);
            }
            Stmt::Block(stmts, _) => {
                for s in stmts {
                    self.gen_stmt(s, func);
                }
            }
            Stmt::Return(maybe_expr, s) => {
                if let Some(expr) = maybe_expr {
                    self.gen_expr(expr, func);
                    // Result is in RAX/EAX
                }
                if let Some(ret_lbl) = &self.current_return_label.clone() {
                    self.emit_inst(
                        &format!("jmp {}", ret_lbl),
                        AsmInstructionKind::Branch,
                        Some("Jump to function return epilogue"),
                        Some(*s),
                    );
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
                span: s,
            } => {
                let else_lbl = self.new_label("else");
                let end_lbl = self.new_label("if_end");

                self.gen_expr(cond, func);
                self.emit_inst(
                    "cmp rax, 0",
                    AsmInstructionKind::Alu,
                    Some("Test if-condition"),
                    Some(*s),
                );

                if else_branch.is_some() {
                    self.emit_inst(
                        &format!("je {}", else_lbl),
                        AsmInstructionKind::Branch,
                        Some("Branch to else"),
                        Some(*s),
                    );
                } else {
                    self.emit_inst(
                        &format!("je {}", end_lbl),
                        AsmInstructionKind::Branch,
                        Some("Branch past if-block"),
                        Some(*s),
                    );
                }

                self.gen_stmt(then_branch, func);

                if let Some(else_stmt) = else_branch {
                    self.emit_inst(
                        &format!("jmp {}", end_lbl),
                        AsmInstructionKind::Branch,
                        Some("Jump past else"),
                        Some(*s),
                    );
                    self.emit_label(&else_lbl, Some("Else branch"));
                    self.gen_stmt(else_stmt, func);
                }

                self.emit_label(&end_lbl, Some("End of if"));
            }
            Stmt::While { cond, body, span: s } => {
                let start_lbl = self.new_label("while_start");
                let end_lbl = self.new_label("while_end");

                let old_break = self.current_break_label.take();
                let old_continue = self.current_continue_label.take();
                self.current_break_label = Some(end_lbl.clone());
                self.current_continue_label = Some(start_lbl.clone());

                self.emit_label(&start_lbl, Some("While loop start"));
                self.gen_expr(cond, func);
                self.emit_inst(
                    "cmp rax, 0",
                    AsmInstructionKind::Alu,
                    Some("Test loop condition"),
                    Some(*s),
                );
                self.emit_inst(
                    &format!("je {}", end_lbl),
                    AsmInstructionKind::Branch,
                    Some("Exit while loop if false"),
                    Some(*s),
                );

                self.gen_stmt(body, func);
                self.emit_inst(
                    &format!("jmp {}", start_lbl),
                    AsmInstructionKind::Branch,
                    Some("Repeat while loop"),
                    Some(*s),
                );
                self.emit_label(&end_lbl, Some("While loop exit"));

                self.current_break_label = old_break;
                self.current_continue_label = old_continue;
            }
            Stmt::DoWhile { body, cond, span: s } => {
                let start_lbl = self.new_label("dowhile_start");
                let cond_lbl = self.new_label("dowhile_cond");
                let end_lbl = self.new_label("dowhile_end");

                let old_break = self.current_break_label.take();
                let old_continue = self.current_continue_label.take();
                self.current_break_label = Some(end_lbl.clone());
                self.current_continue_label = Some(cond_lbl.clone());

                self.emit_label(&start_lbl, Some("Do-while loop body start"));
                self.gen_stmt(body, func);

                self.emit_label(&cond_lbl, Some("Do-while condition check"));
                self.gen_expr(cond, func);
                self.emit_inst(
                    "cmp rax, 0",
                    AsmInstructionKind::Alu,
                    Some("Test do-while condition"),
                    Some(*s),
                );
                self.emit_inst(
                    &format!("jne {}", start_lbl),
                    AsmInstructionKind::Branch,
                    Some("Repeat loop if true"),
                    Some(*s),
                );
                self.emit_label(&end_lbl, Some("Do-while loop exit"));

                self.current_break_label = old_break;
                self.current_continue_label = old_continue;
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
                span: s,
            } => {
                let start_lbl = self.new_label("for_start");
                let step_lbl = self.new_label("for_step");
                let end_lbl = self.new_label("for_end");

                let old_break = self.current_break_label.take();
                let old_continue = self.current_continue_label.take();
                self.current_break_label = Some(end_lbl.clone());
                self.current_continue_label = Some(step_lbl.clone());

                if let Some(init_stmt) = init {
                    self.gen_stmt(init_stmt, func);
                }

                self.emit_label(&start_lbl, Some("For loop condition"));
                if let Some(cond_expr) = cond {
                    self.gen_expr(cond_expr, func);
                    self.emit_inst(
                        "cmp rax, 0",
                        AsmInstructionKind::Alu,
                        Some("Test for-loop condition"),
                        Some(*s),
                    );
                    self.emit_inst(
                        &format!("je {}", end_lbl),
                        AsmInstructionKind::Branch,
                        Some("Exit for-loop if false"),
                        Some(*s),
                    );
                }

                self.gen_stmt(body, func);

                self.emit_label(&step_lbl, Some("For loop step"));
                if let Some(step_expr) = step {
                    self.gen_expr(step_expr, func);
                }
                self.emit_inst(
                    &format!("jmp {}", start_lbl),
                    AsmInstructionKind::Branch,
                    Some("Repeat for-loop"),
                    Some(*s),
                );
                self.emit_label(&end_lbl, Some("For loop exit"));

                self.current_break_label = old_break;
                self.current_continue_label = old_continue;
            }
            Stmt::Break(s) => {
                if let Some(brk) = &self.current_break_label.clone() {
                    self.emit_inst(
                        &format!("jmp {}", brk),
                        AsmInstructionKind::Branch,
                        Some("Break loop"),
                        Some(*s),
                    );
                }
            }
            Stmt::Continue(s) => {
                if let Some(cont) = &self.current_continue_label.clone() {
                    self.emit_inst(
                        &format!("jmp {}", cont),
                        AsmInstructionKind::Branch,
                        Some("Continue loop"),
                        Some(*s),
                    );
                }
            }
            Stmt::VarDecl(items) => {
                for item in items {
                    if let Some(init_expr) = &item.init {
                        // Compute lvalue address of variable
                        self.gen_lval_by_name(&item.name, func, span);
                        self.emit_inst(
                            "push rax",
                            AsmInstructionKind::StackOp,
                            Some("Save destination address for initializer"),
                            span,
                        );

                        // Compute initial value
                        self.gen_expr(init_expr, func);

                        // Pop destination address
                        self.emit_inst(
                            "pop rdi",
                            AsmInstructionKind::StackOp,
                            Some("Restore destination address"),
                            span,
                        );

                        // Store
                        self.store_to_rdi(&item.ty, span);
                    }
                }
            }
        }
    }

    fn gen_lval_by_name(&mut self, name: &str, func: &FunctionDef, span: Option<SourceSpan>) {
        if let Some(local) = func.locals.iter().find(|l| l.name == name) {
            let offset_str = if local.rbp_offset < 0 {
                format!("rbp{}", local.rbp_offset)
            } else {
                format!("rbp+{}", local.rbp_offset)
            };
            self.emit_inst(
                &format!("lea rax, [{}]", offset_str),
                AsmInstructionKind::Alu,
                Some(&format!("Calculate address of local variable '{}'", name)),
                span,
            );
        } else {
            // Global variable
            self.emit_inst(
                &format!("lea rax, [{}]", name),
                AsmInstructionKind::Alu,
                Some(&format!("Calculate address of global variable '{}'", name)),
                span,
            );
        }
    }

    fn gen_lval(&mut self, expr: &Expr, func: &FunctionDef) {
        let span = Some(expr.span());
        match expr {
            Expr::Variable(name, _, _) => {
                self.gen_lval_by_name(name, func, span);
            }
            Expr::Unary {
                op: UnaryOp::Deref,
                expr: inner,
                ..
            } => {
                // Address of *p is p!
                self.gen_expr(inner, func);
            }
            Expr::MemberAccess {
                expr: base,
                offset,
                member,
                ..
            } => {
                self.gen_lval(base, func);
                if *offset > 0 {
                    self.emit_inst(
                        &format!("add rax, {}", offset),
                        AsmInstructionKind::Alu,
                        Some(&format!("Add offset {} for member '{}'", offset, member)),
                        span,
                    );
                }
            }
            _ => {
                // Fallback: evaluate expression into rax
                self.gen_expr(expr, func);
            }
        }
    }

    fn load_from_rax(&mut self, ty: &Type, span: Option<SourceSpan>) {
        match ty {
            Type::Array(_, _) | Type::Struct { .. } => {
                // Array and Struct evaluate to their address in rax; no scalar load needed
            }
            Type::Char => {
                self.emit_inst(
                    "movsx eax, BYTE PTR [rax]",
                    AsmInstructionKind::MemoryRead,
                    Some("Dereference 1-byte char with sign extension"),
                    span,
                );
            }
            Type::Short => {
                self.emit_inst(
                    "movsx eax, WORD PTR [rax]",
                    AsmInstructionKind::MemoryRead,
                    Some("Dereference 2-byte short with sign extension"),
                    span,
                );
            }
            Type::Int => {
                self.emit_inst(
                    "mov eax, DWORD PTR [rax]",
                    AsmInstructionKind::MemoryRead,
                    Some("Dereference 4-byte int"),
                    span,
                );
            }
            Type::Long | Type::Pointer(_) => {
                self.emit_inst(
                    "mov rax, QWORD PTR [rax]",
                    AsmInstructionKind::MemoryRead,
                    Some("Dereference 8-byte pointer/long"),
                    span,
                );
            }
            Type::Void => {}
        }
    }

    fn store_to_rdi(&mut self, ty: &Type, span: Option<SourceSpan>) {
        match ty.size() {
            1 => {
                self.emit_inst(
                    "mov BYTE PTR [rdi], al",
                    AsmInstructionKind::MemoryWrite,
                    Some("Store 1 byte to [RDI]"),
                    span,
                );
            }
            2 => {
                self.emit_inst(
                    "mov WORD PTR [rdi], ax",
                    AsmInstructionKind::MemoryWrite,
                    Some("Store 2 bytes to [RDI]"),
                    span,
                );
            }
            4 => {
                self.emit_inst(
                    "mov DWORD PTR [rdi], eax",
                    AsmInstructionKind::MemoryWrite,
                    Some("Store 4 bytes to [RDI]"),
                    span,
                );
            }
            _ => {
                self.emit_inst(
                    "mov QWORD PTR [rdi], rax",
                    AsmInstructionKind::MemoryWrite,
                    Some("Store 8 bytes to [RDI]"),
                    span,
                );
            }
        }
    }

    pub fn gen_expr(&mut self, expr: &Expr, func: &FunctionDef) {
        let span = Some(expr.span());

        match expr {
            Expr::Number(n, _, _) => {
                self.emit_inst(
                    &format!("mov rax, {}", n),
                    AsmInstructionKind::Alu,
                    Some(&format!("Load constant {}", n)),
                    span,
                );
            }
            Expr::StringLiteral(id, _) => {
                self.emit_inst(
                    &format!("lea rax, [.L.str.{}]", id),
                    AsmInstructionKind::Alu,
                    Some("Load address of string literal"),
                    span,
                );
            }
            Expr::Variable(name, ty, _) => {
                self.gen_lval_by_name(name, func, span);
                self.load_from_rax(ty, span);
            }
            Expr::Assign { target, value, ty, .. } => {
                self.gen_lval(target, func);
                self.emit_inst(
                    "push rax",
                    AsmInstructionKind::StackOp,
                    Some("Save target address for assignment"),
                    span,
                );

                self.gen_expr(value, func);

                self.emit_inst(
                    "pop rdi",
                    AsmInstructionKind::StackOp,
                    Some("Restore target address"),
                    span,
                );
                self.store_to_rdi(ty, span);
            }
            Expr::CompoundAssign {
                op,
                target,
                value,
                ty,
                ..
            } => {
                self.gen_lval(target, func);
                self.emit_inst(
                    "push rax",
                    AsmInstructionKind::StackOp,
                    Some("Save target address"),
                    span,
                );

                // Load existing value
                self.load_from_rax(ty, span);
                self.emit_inst(
                    "push rax",
                    AsmInstructionKind::StackOp,
                    Some("Save current value for compound op"),
                    span,
                );

                // Compute rhs
                self.gen_expr(value, func);
                self.emit_inst(
                    "mov rdi, rax",
                    AsmInstructionKind::Alu,
                    Some("Move rhs to RDI"),
                    span,
                );
                self.emit_inst(
                    "pop rax",
                    AsmInstructionKind::StackOp,
                    Some("Pop lhs into RAX"),
                    span,
                );

                // Perform binary op
                self.gen_binary_op(*op, span);

                // Store back
                self.emit_inst(
                    "pop rdi",
                    AsmInstructionKind::StackOp,
                    Some("Pop target address into RDI"),
                    span,
                );
                self.store_to_rdi(ty, span);
            }
            Expr::Binary {
                op,
                left,
                right,
                ..
            } => {
                if *op == BinaryOp::LogicalAnd {
                    let false_lbl = self.new_label("land_false");
                    let end_lbl = self.new_label("land_end");

                    self.gen_expr(left, func);
                    self.emit_inst("cmp rax, 0", AsmInstructionKind::Alu, None, span);
                    self.emit_inst(
                        &format!("je {}", false_lbl),
                        AsmInstructionKind::Branch,
                        Some("Short-circuit logical AND"),
                        span,
                    );

                    self.gen_expr(right, func);
                    self.emit_inst("cmp rax, 0", AsmInstructionKind::Alu, None, span);
                    self.emit_inst(&format!("je {}", false_lbl), AsmInstructionKind::Branch, None, span);

                    self.emit_inst("mov rax, 1", AsmInstructionKind::Alu, None, span);
                    self.emit_inst(&format!("jmp {}", end_lbl), AsmInstructionKind::Branch, None, span);

                    self.emit_label(&false_lbl, None);
                    self.emit_inst("mov rax, 0", AsmInstructionKind::Alu, None, span);
                    self.emit_label(&end_lbl, None);
                    return;
                }

                if *op == BinaryOp::LogicalOr {
                    let true_lbl = self.new_label("lor_true");
                    let end_lbl = self.new_label("lor_end");

                    self.gen_expr(left, func);
                    self.emit_inst("cmp rax, 0", AsmInstructionKind::Alu, None, span);
                    self.emit_inst(
                        &format!("jne {}", true_lbl),
                        AsmInstructionKind::Branch,
                        Some("Short-circuit logical OR"),
                        span,
                    );

                    self.gen_expr(right, func);
                    self.emit_inst("cmp rax, 0", AsmInstructionKind::Alu, None, span);
                    self.emit_inst(&format!("jne {}", true_lbl), AsmInstructionKind::Branch, None, span);

                    self.emit_inst("mov rax, 0", AsmInstructionKind::Alu, None, span);
                    self.emit_inst(&format!("jmp {}", end_lbl), AsmInstructionKind::Branch, None, span);

                    self.emit_label(&true_lbl, None);
                    self.emit_inst("mov rax, 1", AsmInstructionKind::Alu, None, span);
                    self.emit_label(&end_lbl, None);
                    return;
                }

                // General binary operation:
                self.gen_expr(left, func);
                self.emit_inst(
                    "push rax",
                    AsmInstructionKind::StackOp,
                    Some("Save left operand on stack"),
                    span,
                );

                self.gen_expr(right, func);
                self.emit_inst(
                    "mov rdi, rax",
                    AsmInstructionKind::Alu,
                    Some("Move right operand to RDI"),
                    span,
                );
                self.emit_inst(
                    "pop rax",
                    AsmInstructionKind::StackOp,
                    Some("Restore left operand into RAX"),
                    span,
                );

                self.gen_binary_op(*op, span);
            }
            Expr::Unary { op, expr: inner, .. } => match op {
                UnaryOp::Neg => {
                    self.gen_expr(inner, func);
                    self.emit_inst("neg rax", AsmInstructionKind::Alu, Some("Negate RAX"), span);
                }
                UnaryOp::Pos => {
                    self.gen_expr(inner, func);
                }
                UnaryOp::Not => {
                    self.gen_expr(inner, func);
                    self.emit_inst("cmp rax, 0", AsmInstructionKind::Alu, None, span);
                    self.emit_inst(
                        "sete al",
                        AsmInstructionKind::Alu,
                        Some("Set AL to 1 if RAX == 0, else 0"),
                        span,
                    );
                    self.emit_inst(
                        "movzx rax, al",
                        AsmInstructionKind::Alu,
                        Some("Zero extend AL into RAX"),
                        span,
                    );
                }
                UnaryOp::BitNot => {
                    self.gen_expr(inner, func);
                    self.emit_inst("not rax", AsmInstructionKind::Alu, Some("Bitwise NOT RAX"), span);
                }
                UnaryOp::Deref => {
                    self.gen_expr(inner, func);
                    self.load_from_rax(&expr.get_type(), span);
                }
                UnaryOp::AddrOf => {
                    self.gen_lval(inner, func);
                }
                UnaryOp::PreInc => {
                    self.gen_lval(inner, func);
                    self.emit_inst("push rax", AsmInstructionKind::StackOp, None, span);
                    self.load_from_rax(&inner.get_type(), span);
                    self.emit_inst(
                        "add rax, 1",
                        AsmInstructionKind::Alu,
                        Some("Increment value"),
                        span,
                    );
                    self.emit_inst("pop rdi", AsmInstructionKind::StackOp, None, span);
                    self.store_to_rdi(&inner.get_type(), span);
                }
                UnaryOp::PreDec => {
                    self.gen_lval(inner, func);
                    self.emit_inst("push rax", AsmInstructionKind::StackOp, None, span);
                    self.load_from_rax(&inner.get_type(), span);
                    self.emit_inst(
                        "sub rax, 1",
                        AsmInstructionKind::Alu,
                        Some("Decrement value"),
                        span,
                    );
                    self.emit_inst("pop rdi", AsmInstructionKind::StackOp, None, span);
                    self.store_to_rdi(&inner.get_type(), span);
                }
                UnaryOp::PostInc => {
                    self.gen_lval(inner, func);
                    self.emit_inst("push rax", AsmInstructionKind::StackOp, None, span);
                    self.load_from_rax(&inner.get_type(), span);
                    self.emit_inst(
                        "push rax",
                        AsmInstructionKind::StackOp,
                        Some("Save original value for post-increment result"),
                        span,
                    );
                    self.emit_inst(
                        "add rax, 1",
                        AsmInstructionKind::Alu,
                        Some("Increment value"),
                        span,
                    );
                    self.emit_inst(
                        "mov rdi, [rsp+8]",
                        AsmInstructionKind::MemoryRead,
                        Some("Get variable address from stack"),
                        span,
                    );
                    self.store_to_rdi(&inner.get_type(), span);
                    self.emit_inst(
                        "pop rax",
                        AsmInstructionKind::StackOp,
                        Some("Restore pre-increment value as expression result"),
                        span,
                    );
                    self.emit_inst(
                        "add rsp, 8",
                        AsmInstructionKind::StackOp,
                        Some("Clean up variable address from stack"),
                        span,
                    );
                }
                UnaryOp::PostDec => {
                    self.gen_lval(inner, func);
                    self.emit_inst("push rax", AsmInstructionKind::StackOp, None, span);
                    self.load_from_rax(&inner.get_type(), span);
                    self.emit_inst(
                        "push rax",
                        AsmInstructionKind::StackOp,
                        Some("Save original value for post-decrement result"),
                        span,
                    );
                    self.emit_inst(
                        "sub rax, 1",
                        AsmInstructionKind::Alu,
                        Some("Decrement value"),
                        span,
                    );
                    self.emit_inst(
                        "mov rdi, [rsp+8]",
                        AsmInstructionKind::MemoryRead,
                        Some("Get variable address from stack"),
                        span,
                    );
                    self.store_to_rdi(&inner.get_type(), span);
                    self.emit_inst(
                        "pop rax",
                        AsmInstructionKind::StackOp,
                        Some("Restore pre-decrement value as expression result"),
                        span,
                    );
                    self.emit_inst(
                        "add rsp, 8",
                        AsmInstructionKind::StackOp,
                        Some("Clean up variable address from stack"),
                        span,
                    );
                }
            },
            Expr::Call { callee, args, .. } => {
                let arg_regs_64 = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];

                // Evaluate args and push them onto stack
                for arg in args {
                    self.gen_expr(arg, func);
                    self.emit_inst(
                        "push rax",
                        AsmInstructionKind::StackOp,
                        Some("Push function argument to stack"),
                        span,
                    );
                }

                // Pop arguments into registers in reverse order
                for i in (0..args.len().min(6)).rev() {
                    self.emit_inst(
                        &format!("pop {}", arg_regs_64[i]),
                        AsmInstructionKind::StackOp,
                        Some(&format!("Pop argument into register {}", arg_regs_64[i].to_uppercase())),
                        span,
                    );
                }

                // For variadic calls (like printf), AL specifies number of vector registers used
                self.emit_inst(
                    "mov al, 0",
                    AsmInstructionKind::Alu,
                    Some("0 vector registers used for call"),
                    span,
                );

                self.emit_inst(
                    &format!("call {}", callee),
                    AsmInstructionKind::Call,
                    Some(&format!("Call function '{}'", callee)),
                    span,
                );
            }
            Expr::Comma { left, right, .. } => {
                self.gen_expr(left, func);
                self.gen_expr(right, func);
            }
            Expr::Cast { expr: inner, .. } => {
                self.gen_expr(inner, func);
            }
            Expr::MemberAccess { ty, .. } => {
                self.gen_lval(expr, func);
                self.load_from_rax(ty, span);
            }
        }
    }

    fn gen_binary_op(&mut self, op: BinaryOp, span: Option<SourceSpan>) {
        match op {
            BinaryOp::Add => {
                self.emit_inst(
                    "add rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Add RDI to RAX"),
                    span,
                );
            }
            BinaryOp::Sub => {
                self.emit_inst(
                    "sub rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Subtract RDI from RAX"),
                    span,
                );
            }
            BinaryOp::Mul => {
                self.emit_inst(
                    "imul rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Signed multiply RAX by RDI"),
                    span,
                );
            }
            BinaryOp::Div => {
                self.emit_inst(
                    "cqo",
                    AsmInstructionKind::Alu,
                    Some("Sign extend RAX into RDX:RAX for division"),
                    span,
                );
                self.emit_inst(
                    "idiv rdi",
                    AsmInstructionKind::Alu,
                    Some("Divide RDX:RAX by RDI (quotient in RAX)"),
                    span,
                );
            }
            BinaryOp::Mod => {
                self.emit_inst(
                    "cqo",
                    AsmInstructionKind::Alu,
                    Some("Sign extend RAX into RDX:RAX for remainder"),
                    span,
                );
                self.emit_inst(
                    "idiv rdi",
                    AsmInstructionKind::Alu,
                    Some("Divide RDX:RAX by RDI (remainder in RDX)"),
                    span,
                );
                self.emit_inst(
                    "mov rax, rdx",
                    AsmInstructionKind::Alu,
                    Some("Move remainder from RDX to RAX"),
                    span,
                );
            }
            BinaryOp::BitAnd => {
                self.emit_inst(
                    "and rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Bitwise AND RAX with RDI"),
                    span,
                );
            }
            BinaryOp::BitOr => {
                self.emit_inst(
                    "or rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Bitwise OR RAX with RDI"),
                    span,
                );
            }
            BinaryOp::BitXor => {
                self.emit_inst(
                    "xor rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Bitwise XOR RAX with RDI"),
                    span,
                );
            }
            BinaryOp::ShiftLeft => {
                self.emit_inst(
                    "mov rcx, rdi",
                    AsmInstructionKind::Alu,
                    Some("Move shift count to RCX"),
                    span,
                );
                self.emit_inst(
                    "shl rax, cl",
                    AsmInstructionKind::Alu,
                    Some("Shift left RAX by CL bits"),
                    span,
                );
            }
            BinaryOp::ShiftRight => {
                self.emit_inst(
                    "mov rcx, rdi",
                    AsmInstructionKind::Alu,
                    Some("Move shift count to RCX"),
                    span,
                );
                self.emit_inst(
                    "sar rax, cl",
                    AsmInstructionKind::Alu,
                    Some("Arithmetic shift right RAX by CL bits"),
                    span,
                );
            }
            BinaryOp::Equal => {
                self.emit_inst(
                    "cmp rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Compare RAX with RDI"),
                    span,
                );
                self.emit_inst(
                    "sete al",
                    AsmInstructionKind::Alu,
                    Some("Set AL to 1 if equal, else 0"),
                    span,
                );
                self.emit_inst(
                    "movzx rax, al",
                    AsmInstructionKind::Alu,
                    Some("Zero extend AL to RAX"),
                    span,
                );
            }
            BinaryOp::NotEqual => {
                self.emit_inst(
                    "cmp rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Compare RAX with RDI"),
                    span,
                );
                self.emit_inst(
                    "setne al",
                    AsmInstructionKind::Alu,
                    Some("Set AL to 1 if not equal, else 0"),
                    span,
                );
                self.emit_inst(
                    "movzx rax, al",
                    AsmInstructionKind::Alu,
                    Some("Zero extend AL to RAX"),
                    span,
                );
            }
            BinaryOp::Less => {
                self.emit_inst(
                    "cmp rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Compare RAX with RDI"),
                    span,
                );
                self.emit_inst(
                    "setl al",
                    AsmInstructionKind::Alu,
                    Some("Set AL to 1 if less, else 0"),
                    span,
                );
                self.emit_inst(
                    "movzx rax, al",
                    AsmInstructionKind::Alu,
                    Some("Zero extend AL to RAX"),
                    span,
                );
            }
            BinaryOp::LessEqual => {
                self.emit_inst(
                    "cmp rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Compare RAX with RDI"),
                    span,
                );
                self.emit_inst(
                    "setle al",
                    AsmInstructionKind::Alu,
                    Some("Set AL to 1 if less or equal, else 0"),
                    span,
                );
                self.emit_inst(
                    "movzx rax, al",
                    AsmInstructionKind::Alu,
                    Some("Zero extend AL to RAX"),
                    span,
                );
            }
            BinaryOp::Greater => {
                self.emit_inst(
                    "cmp rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Compare RAX with RDI"),
                    span,
                );
                self.emit_inst(
                    "setg al",
                    AsmInstructionKind::Alu,
                    Some("Set AL to 1 if greater, else 0"),
                    span,
                );
                self.emit_inst(
                    "movzx rax, al",
                    AsmInstructionKind::Alu,
                    Some("Zero extend AL to RAX"),
                    span,
                );
            }
            BinaryOp::GreaterEqual => {
                self.emit_inst(
                    "cmp rax, rdi",
                    AsmInstructionKind::Alu,
                    Some("Compare RAX with RDI"),
                    span,
                );
                self.emit_inst(
                    "setge al",
                    AsmInstructionKind::Alu,
                    Some("Set AL to 1 if greater or equal, else 0"),
                    span,
                );
                self.emit_inst(
                    "movzx rax, al",
                    AsmInstructionKind::Alu,
                    Some("Zero extend AL to RAX"),
                    span,
                );
            }
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr => {
                // Handled earlier with short-circuit evaluation
            }
        }
    }
}
