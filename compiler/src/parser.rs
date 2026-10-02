use crate::ast::*;
use crate::metadata::{CompileError, SourceSpan};
use crate::token::{Token, TokenKind};

use std::collections::HashMap;

pub struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    string_literals: Vec<(usize, Vec<u8>)>,
    next_string_id: usize,
    // Scope tracking
    current_locals: Vec<LocalVar>,
    current_stack_offset: i32,
    globals: Vec<GlobalVar>,
    struct_types: HashMap<String, Type>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            cursor: 0,
            string_literals: Vec::new(),
            next_string_id: 0,
            current_locals: Vec::new(),
            current_stack_offset: 0,
            globals: Vec::new(),
            struct_types: HashMap::new(),
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.cursor].kind
    }

    fn peek_ahead(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.cursor + offset)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.cursor];
        if self.cursor + 1 < self.tokens.len() {
            self.cursor += 1;
        }
        tok
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind, msg: &str) -> Result<Token, CompileError> {
        if self.check(kind) {
            Ok(self.advance().clone())
        } else {
            let tok = self.peek();
            Err(CompileError {
                message: format!("Expected {}, found {:?}", msg, tok.kind),
                line: tok.span.start_line,
                column: tok.span.start_col,
            })
        }
    }

    fn allocate_local_var(&mut self, name: &str, ty: Type) -> LocalVar {
        let size = ty.size();
        let align = ty.alignment();

        // Stack grows downward. Offset = current_stack_offset - size, aligned.
        let mut offset = self.current_stack_offset - (size as i32);
        // Align offset downwards to multiple of `align`
        let rem = offset.rem_euclid(align as i32);
        if rem != 0 {
            offset -= rem;
        }
        self.current_stack_offset = offset;

        let local = LocalVar {
            name: name.to_string(),
            ty,
            rbp_offset: offset,
            size,
        };
        self.current_locals.push(local.clone());
        local
    }

    fn find_local_var(&self, name: &str) -> Option<&LocalVar> {
        self.current_locals.iter().rev().find(|v| v.name == name)
    }

    fn find_global_var(&self, name: &str) -> Option<&GlobalVar> {
        self.globals.iter().find(|g| g.name == name)
    }

    pub fn parse_program(&mut self) -> Result<Program, CompileError> {
        let mut functions = Vec::new();

        while !self.check(&TokenKind::Eof) {
            let base_type = self.parse_base_type()?;
            if self.match_token(&TokenKind::Semicolon) {
                // Standalone struct or type declaration, e.g. `struct Point { int x, y; };`
                continue;
            }

            let (ty, name, span) = self.parse_declarator(base_type)?;

            if self.check(&TokenKind::LParen) {
                // Function definition or declaration
                let func = self.parse_function_definition(ty, name, span)?;
                if let Some(f) = func {
                    functions.push(f);
                }
            } else {
                // Global variable declaration
                self.parse_global_variable(ty, name, span)?;
            }
        }

        Ok(Program {
            globals: self.globals.clone(),
            functions,
            string_literals: self.string_literals.clone(),
        })
    }

    fn is_type_specifier(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Int
                | TokenKind::Char
                | TokenKind::Short
                | TokenKind::Long
                | TokenKind::Void
                | TokenKind::Struct
        )
    }

    fn parse_base_type(&mut self) -> Result<Type, CompileError> {
        let tok = self.advance().clone();
        match tok.kind {
            TokenKind::Void => Ok(Type::Void),
            TokenKind::Char => Ok(Type::Char),
            TokenKind::Short => Ok(Type::Short),
            TokenKind::Int => Ok(Type::Int),
            TokenKind::Long => Ok(Type::Long),
            TokenKind::Struct => self.parse_struct_specifier(),
            _ => Err(CompileError {
                message: format!("Expected type specifier, found {:?}", tok.kind),
                line: tok.span.start_line,
                column: tok.span.start_col,
            }),
        }
    }

    fn parse_struct_specifier(&mut self) -> Result<Type, CompileError> {
        let tag = if let TokenKind::Ident(ref s) = self.peek_kind() {
            let name = s.clone();
            self.advance();
            Some(name)
        } else {
            None
        };

        if self.match_token(&TokenKind::LBrace) {
            // Register preliminary placeholder for self-referential pointer types
            if let Some(ref t) = tag {
                self.struct_types.insert(
                    t.clone(),
                    Type::Struct {
                        name: Some(t.clone()),
                        members: Vec::new(),
                        size: 0,
                        align: 1,
                    },
                );
            }

            let mut raw_members = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                let mem_base = self.parse_base_type()?;
                loop {
                    let (mem_ty, mem_name, _) = self.parse_declarator(mem_base.clone())?;
                    raw_members.push((mem_name, mem_ty));
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::Semicolon, "';'")?;
            }
            self.expect(&TokenKind::RBrace, "'}'")?;

            let mut current_offset = 0;
            let mut max_align = 1;
            let mut members = Vec::new();

            for (name, ty) in raw_members {
                let align = ty.alignment();
                max_align = max_align.max(align);
                if align > 0 {
                    let rem = current_offset % align;
                    if rem != 0 {
                        current_offset += align - rem;
                    }
                }
                members.push(StructMember {
                    name,
                    ty: ty.clone(),
                    offset: current_offset,
                });
                current_offset += ty.size();
            }

            let total_size = if max_align > 0 {
                let rem = current_offset % max_align;
                if rem != 0 {
                    current_offset + (max_align - rem)
                } else {
                    current_offset
                }
            } else {
                current_offset
            };

            let struct_type = Type::Struct {
                name: tag.clone(),
                members,
                size: total_size,
                align: max_align,
            };

            if let Some(ref t) = tag {
                self.struct_types.insert(t.clone(), struct_type.clone());
            }

            Ok(struct_type)
        } else if let Some(ref t) = tag {
            match self.struct_types.get(t) {
                Some(ty) => Ok(ty.clone()),
                None => {
                    let tok = self.peek();
                    Err(CompileError {
                        message: format!("Undefined struct 'struct {}'", t),
                        line: tok.span.start_line,
                        column: tok.span.start_col,
                    })
                }
            }
        } else {
            let tok = self.peek();
            Err(CompileError {
                message: "Expected struct tag or '{'".to_string(),
                line: tok.span.start_line,
                column: tok.span.start_col,
            })
        }
    }

    fn parse_declarator(&mut self, mut base_type: Type) -> Result<(Type, String, SourceSpan), CompileError> {
        // Handle pointer prefixes (*, **, etc.)
        while self.match_token(&TokenKind::Star) {
            base_type = Type::Pointer(Box::new(base_type));
        }

        let (name, span) = match self.peek_kind() {
            TokenKind::Ident(ident) => {
                let s = ident.clone();
                let span = self.peek().span;
                self.advance();
                (s, span)
            }
            _ => {
                let tok = self.peek();
                return Err(CompileError {
                    message: format!("Expected identifier in declaration, found {:?}", tok.kind),
                    line: tok.span.start_line,
                    column: tok.span.start_col,
                });
            }
        };

        // Handle array suffixes ([N], [N][M], etc.)
        let mut dimensions = Vec::new();
        while self.match_token(&TokenKind::LBracket) {
            let num = match self.advance().kind.clone() {
                TokenKind::Number(n) if n > 0 => n as usize,
                other => {
                    return Err(CompileError {
                        message: format!("Expected positive array size, found {:?}", other),
                        line: span.start_line,
                        column: span.start_col,
                    })
                }
            };
            self.expect(&TokenKind::RBracket, "']'")?;
            dimensions.push(num);
        }

        // Apply dimensions from right to left (innermost to outermost)
        // e.g. int matrix[2][3] -> Type::Array(Type::Array(int, 3), 2)
        for &dim in dimensions.iter().rev() {
            base_type = Type::Array(Box::new(base_type), dim);
        }

        Ok((base_type, name, span))
    }

    fn parse_function_definition(
        &mut self,
        ret_type: Type,
        name: String,
        span: SourceSpan,
    ) -> Result<Option<FunctionDef>, CompileError> {
        self.expect(&TokenKind::LParen, "'('")?;

        // Reset local scope for new function
        self.current_locals.clear();
        self.current_stack_offset = 0;

        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                if self.check(&TokenKind::Void) && self.peek_ahead(1).map(|t| &t.kind) == Some(&TokenKind::RParen) {
                    self.advance();
                    break;
                }

                let param_base = self.parse_base_type()?;
                let (param_type, param_name, _) = self.parse_declarator(param_base)?;

                // Allocate stack slot for parameter
                self.allocate_local_var(&param_name, param_type.clone());
                params.push((param_name, param_type));

                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "')'")?;

        // Check if declaration only (e.g. `int foo();`)
        if self.match_token(&TokenKind::Semicolon) {
            return Ok(None);
        }

        // Body must be a block
        let body_stmt = self.parse_block_statement()?;
        let body = match body_stmt {
            Stmt::Block(stmts, _) => stmts,
            other => vec![other],
        };

        // Align total stack frame size to 16 bytes (AMD64 requirement)
        let raw_size = (-self.current_stack_offset) as usize;
        let stack_size = (raw_size + 15) & !15;

        Ok(Some(FunctionDef {
            name,
            ret_type,
            params,
            body,
            locals: self.current_locals.clone(),
            stack_size,
            span,
        }))
    }

    fn parse_global_variable(
        &mut self,
        first_ty: Type,
        first_name: String,
        _first_span: SourceSpan,
    ) -> Result<(), CompileError> {
        let mut ty = first_ty;
        let mut name = first_name;

        loop {
            let init_val = if self.match_token(&TokenKind::Equal) {
                match self.advance().kind.clone() {
                    TokenKind::Number(n) => GlobalInit::Number(n),
                    TokenKind::StringLiteral(bytes) => {
                        let id = self.next_string_id;
                        self.next_string_id += 1;
                        self.string_literals.push((id, bytes));
                        GlobalInit::StringLiteral(id)
                    }
                    other => {
                        return Err(CompileError {
                            message: format!(
                                "Global variable initializers must be constant numbers or string literals, found {:?}",
                                other
                            ),
                            line: self.peek().span.start_line,
                            column: self.peek().span.start_col,
                        })
                    }
                }
            } else {
                GlobalInit::None
            };

            self.globals.push(GlobalVar {
                name,
                ty,
                init_val,
            });

            if self.match_token(&TokenKind::Comma) {
                let base = self.globals.last().unwrap().ty.clone();
                let (next_ty, next_name, _) = self.parse_declarator(base)?;
                ty = next_ty;
                name = next_name;
            } else {
                break;
            }
        }

        self.expect(&TokenKind::Semicolon, "';'")?;
        Ok(())
    }

    fn parse_statement(&mut self) -> Result<Stmt, CompileError> {
        match self.peek_kind() {
            TokenKind::LBrace => self.parse_block_statement(),
            TokenKind::Return => self.parse_return_statement(),
            TokenKind::If => self.parse_if_statement(),
            TokenKind::While => self.parse_while_statement(),
            TokenKind::Do => self.parse_do_while_statement(),
            TokenKind::For => self.parse_for_statement(),
            TokenKind::Break => {
                let span = self.advance().span;
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(Stmt::Break(span))
            }
            TokenKind::Continue => {
                let span = self.advance().span;
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(Stmt::Continue(span))
            }
            _ if self.is_type_specifier() => self.parse_var_decl_statement(),
            _ => {
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_block_statement(&mut self) -> Result<Stmt, CompileError> {
        let start_span = self.expect(&TokenKind::LBrace, "'{'")?.span;
        let mut stmts = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            stmts.push(self.parse_statement()?);
        }

        let end_span = self.expect(&TokenKind::RBrace, "'}'")?.span;
        Ok(Stmt::Block(
            stmts,
            SourceSpan {
                start_line: start_span.start_line,
                start_col: start_span.start_col,
                end_line: end_span.end_line,
                end_col: end_span.end_col,
            },
        ))
    }

    fn parse_return_statement(&mut self) -> Result<Stmt, CompileError> {
        let start_span = self.advance().span;
        let expr = if !self.check(&TokenKind::Semicolon) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        let end_span = self.expect(&TokenKind::Semicolon, "';'")?.span;
        Ok(Stmt::Return(
            expr,
            SourceSpan {
                start_line: start_span.start_line,
                start_col: start_span.start_col,
                end_line: end_span.end_line,
                end_col: end_span.end_col,
            },
        ))
    }

    fn parse_if_statement(&mut self) -> Result<Stmt, CompileError> {
        let start_span = self.advance().span;
        self.expect(&TokenKind::LParen, "'('")?;
        let cond = self.parse_expression()?;
        self.expect(&TokenKind::RParen, "')'")?;

        let then_branch = Box::new(self.parse_statement()?);
        let else_branch = if self.match_token(&TokenKind::Else) {
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };

        let end_line = else_branch.as_ref().map(|b| b.span().end_line).unwrap_or(then_branch.span().end_line);
        let end_col = else_branch.as_ref().map(|b| b.span().end_col).unwrap_or(then_branch.span().end_col);

        Ok(Stmt::If {
            cond,
            then_branch,
            else_branch,
            span: SourceSpan {
                start_line: start_span.start_line,
                start_col: start_span.start_col,
                end_line,
                end_col,
            },
        })
    }

    fn parse_while_statement(&mut self) -> Result<Stmt, CompileError> {
        let start_span = self.advance().span;
        self.expect(&TokenKind::LParen, "'('")?;
        let cond = self.parse_expression()?;
        self.expect(&TokenKind::RParen, "')'")?;
        let body = Box::new(self.parse_statement()?);

        Ok(Stmt::While {
            cond,
            span: SourceSpan {
                start_line: start_span.start_line,
                start_col: start_span.start_col,
                end_line: body.span().end_line,
                end_col: body.span().end_col,
            },
            body,
        })
    }

    fn parse_do_while_statement(&mut self) -> Result<Stmt, CompileError> {
        let start_span = self.advance().span;
        let body = Box::new(self.parse_statement()?);
        self.expect(&TokenKind::While, "'while'")?;
        self.expect(&TokenKind::LParen, "'('")?;
        let cond = self.parse_expression()?;
        self.expect(&TokenKind::RParen, "')'")?;
        let end_span = self.expect(&TokenKind::Semicolon, "';'")?.span;

        Ok(Stmt::DoWhile {
            body,
            cond,
            span: SourceSpan {
                start_line: start_span.start_line,
                start_col: start_span.start_col,
                end_line: end_span.end_line,
                end_col: end_span.end_col,
            },
        })
    }

    fn parse_for_statement(&mut self) -> Result<Stmt, CompileError> {
        let start_span = self.advance().span;
        self.expect(&TokenKind::LParen, "'('")?;

        let init = if self.match_token(&TokenKind::Semicolon) {
            None
        } else if self.is_type_specifier() {
            Some(Box::new(self.parse_var_decl_statement()?))
        } else {
            let expr = self.parse_expression()?;
            self.expect(&TokenKind::Semicolon, "';'")?;
            Some(Box::new(Stmt::Expr(expr)))
        };

        let cond = if !self.check(&TokenKind::Semicolon) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "';'")?;

        let step = if !self.check(&TokenKind::RParen) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        self.expect(&TokenKind::RParen, "')'")?;

        let body = Box::new(self.parse_statement()?);

        Ok(Stmt::For {
            init,
            cond,
            step,
            span: SourceSpan {
                start_line: start_span.start_line,
                start_col: start_span.start_col,
                end_line: body.span().end_line,
                end_col: body.span().end_col,
            },
            body,
        })
    }

    fn parse_var_decl_statement(&mut self) -> Result<Stmt, CompileError> {
        let base_ty = self.parse_base_type()?;
        if self.match_token(&TokenKind::Semicolon) {
            return Ok(Stmt::VarDecl(Vec::new()));
        }
        let mut items = Vec::new();

        loop {
            let (ty, name, span) = self.parse_declarator(base_ty.clone())?;

            // Allocate stack variable
            self.allocate_local_var(&name, ty.clone());

            let init = if self.match_token(&TokenKind::Equal) {
                Some(self.parse_assignment()?)
            } else {
                None
            };

            items.push(VarDeclItem {
                name,
                ty,
                init,
                span,
            });

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        self.expect(&TokenKind::Semicolon, "';'")?;
        Ok(Stmt::VarDecl(items))
    }

    // Expressions
    pub fn parse_expression(&mut self) -> Result<Expr, CompileError> {
        self.parse_comma()
    }

    fn parse_comma(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.parse_assignment()?;
        while self.match_token(&TokenKind::Comma) {
            let right = self.parse_assignment()?;
            let ty = right.get_type();
            let span = SourceSpan {
                start_line: expr.span().start_line,
                start_col: expr.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            expr = Expr::Comma {
                left: Box::new(expr),
                right: Box::new(right),
                ty,
                span,
            };
        }
        Ok(expr)
    }

    fn parse_assignment(&mut self) -> Result<Expr, CompileError> {
        let expr = self.parse_logical_or()?;

        let compound_op = match self.peek_kind() {
            TokenKind::Equal => None,
            TokenKind::PlusEqual => Some(BinaryOp::Add),
            TokenKind::MinusEqual => Some(BinaryOp::Sub),
            TokenKind::StarEqual => Some(BinaryOp::Mul),
            TokenKind::SlashEqual => Some(BinaryOp::Div),
            TokenKind::PercentEqual => Some(BinaryOp::Mod),
            TokenKind::AmpEqual => Some(BinaryOp::BitAnd),
            TokenKind::PipeEqual => Some(BinaryOp::BitOr),
            TokenKind::CaretEqual => Some(BinaryOp::BitXor),
            TokenKind::ShiftLeftEqual => Some(BinaryOp::ShiftLeft),
            TokenKind::ShiftRightEqual => Some(BinaryOp::ShiftRight),
            _ => return Ok(expr),
        };

        self.advance(); // consume assignment operator
        let value = self.parse_assignment()?;
        let ty = expr.get_type();
        let span = SourceSpan {
            start_line: expr.span().start_line,
            start_col: expr.span().start_col,
            end_line: value.span().end_line,
            end_col: value.span().end_col,
        };

        if let Some(op) = compound_op {
            Ok(Expr::CompoundAssign {
                op,
                target: Box::new(expr),
                value: Box::new(value),
                ty,
                span,
            })
        } else {
            Ok(Expr::Assign {
                target: Box::new(expr),
                value: Box::new(value),
                ty,
                span,
            })
        }
    }

    fn parse_logical_or(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_logical_and()?;
        while self.match_token(&TokenKind::PipePipe) {
            let right = self.parse_logical_and()?;
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op: BinaryOp::LogicalOr,
                left: Box::new(left),
                right: Box::new(right),
                ty: Type::Int,
                span,
            };
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_bitwise_or()?;
        while self.match_token(&TokenKind::AmpAmp) {
            let right = self.parse_bitwise_or()?;
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op: BinaryOp::LogicalAnd,
                left: Box::new(left),
                right: Box::new(right),
                ty: Type::Int,
                span,
            };
        }
        Ok(left)
    }

    fn parse_bitwise_or(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_bitwise_xor()?;
        while self.match_token(&TokenKind::Pipe) {
            let right = self.parse_bitwise_xor()?;
            let ty = left.get_type();
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op: BinaryOp::BitOr,
                left: Box::new(left),
                right: Box::new(right),
                ty,
                span,
            };
        }
        Ok(left)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_bitwise_and()?;
        while self.match_token(&TokenKind::Caret) {
            let right = self.parse_bitwise_and()?;
            let ty = left.get_type();
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(left),
                right: Box::new(right),
                ty,
                span,
            };
        }
        Ok(left)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_equality()?;
        while self.match_token(&TokenKind::Amp) {
            let right = self.parse_equality()?;
            let ty = left.get_type();
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op: BinaryOp::BitAnd,
                left: Box::new(left),
                right: Box::new(right),
                ty,
                span,
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_relational()?;
        while let Some(op) = match self.peek_kind() {
            TokenKind::EqualEqual => Some(BinaryOp::Equal),
            TokenKind::ExclamationEqual => Some(BinaryOp::NotEqual),
            _ => None,
        } {
            self.advance();
            let right = self.parse_relational()?;
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                ty: Type::Int,
                span,
            };
        }
        Ok(left)
    }

    fn parse_relational(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_shift()?;
        while let Some(op) = match self.peek_kind() {
            TokenKind::Less => Some(BinaryOp::Less),
            TokenKind::LessEqual => Some(BinaryOp::LessEqual),
            TokenKind::Greater => Some(BinaryOp::Greater),
            TokenKind::GreaterEqual => Some(BinaryOp::GreaterEqual),
            _ => None,
        } {
            self.advance();
            let right = self.parse_shift()?;
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                ty: Type::Int,
                span,
            };
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_additive()?;
        while let Some(op) = match self.peek_kind() {
            TokenKind::ShiftLeft => Some(BinaryOp::ShiftLeft),
            TokenKind::ShiftRight => Some(BinaryOp::ShiftRight),
            _ => None,
        } {
            self.advance();
            let right = self.parse_additive()?;
            let ty = left.get_type();
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                ty,
                span,
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_multiplicative()?;
        while let Some(op) = match self.peek_kind() {
            TokenKind::Plus => Some(BinaryOp::Add),
            TokenKind::Minus => Some(BinaryOp::Sub),
            _ => None,
        } {
            self.advance();
            let right = self.parse_multiplicative()?;
            let left_ty = left.get_type();
            let right_ty = right.get_type();

            // Pointer arithmetic handling:
            // pointer + int -> pointer (scale int by sizeof(*ptr))
            // int + pointer -> pointer (scale int by sizeof(*ptr))
            // pointer - int -> pointer (scale int by sizeof(*ptr))
            let (res_ty, final_left, final_right) = if left_ty.is_pointer() && right_ty.is_integer() {
                let elem_size = left_ty.base_type().map(|t| t.size()).unwrap_or(1) as i64;
                let scaled_right = if elem_size > 1 {
                    Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(right.clone()),
                        right: Box::new(Expr::Number(elem_size, Type::Int, right.span())),
                        ty: Type::Long,
                        span: right.span(),
                    }
                } else {
                    right
                };
                (left_ty, left, scaled_right)
            } else if op == BinaryOp::Add && left_ty.is_integer() && right_ty.is_pointer() {
                let elem_size = right_ty.base_type().map(|t| t.size()).unwrap_or(1) as i64;
                let scaled_left = if elem_size > 1 {
                    Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(left.clone()),
                        right: Box::new(Expr::Number(elem_size, Type::Int, left.span())),
                        ty: Type::Long,
                        span: left.span(),
                    }
                } else {
                    left
                };
                (right_ty, scaled_left, right)
            } else {
                (left_ty, left, right)
            };

            let span = SourceSpan {
                start_line: final_left.span().start_line,
                start_col: final_left.span().start_col,
                end_line: final_right.span().end_line,
                end_col: final_right.span().end_col,
            };

            left = Expr::Binary {
                op,
                left: Box::new(final_left),
                right: Box::new(final_right),
                ty: res_ty,
                span,
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_unary()?;
        while let Some(op) = match self.peek_kind() {
            TokenKind::Star => Some(BinaryOp::Mul),
            TokenKind::Slash => Some(BinaryOp::Div),
            TokenKind::Percent => Some(BinaryOp::Mod),
            _ => None,
        } {
            self.advance();
            let right = self.parse_unary()?;
            let ty = left.get_type();
            let span = SourceSpan {
                start_line: left.span().start_line,
                start_col: left.span().start_col,
                end_line: right.span().end_line,
                end_col: right.span().end_col,
            };
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                ty,
                span,
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, CompileError> {
        let span = self.peek().span;

        match self.peek_kind() {
            TokenKind::Minus => {
                self.advance();
                let expr = self.parse_unary()?;
                let ty = expr.get_type();
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                    ty,
                    span,
                })
            }
            TokenKind::Plus => {
                self.advance();
                let expr = self.parse_unary()?;
                let ty = expr.get_type();
                Ok(Expr::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(expr),
                    ty,
                    span,
                })
            }
            TokenKind::Exclamation => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                    ty: Type::Int,
                    span,
                })
            }
            TokenKind::Tilde => {
                self.advance();
                let expr = self.parse_unary()?;
                let ty = expr.get_type();
                Ok(Expr::Unary {
                    op: UnaryOp::BitNot,
                    expr: Box::new(expr),
                    ty,
                    span,
                })
            }
            TokenKind::Star => {
                // Dereference: *ptr
                self.advance();
                let expr = self.parse_unary()?;
                let inner_ty = expr.get_type();
                let ty = match inner_ty.base_type() {
                    Some(b) => b.clone(),
                    None => Type::Int,
                };
                Ok(Expr::Unary {
                    op: UnaryOp::Deref,
                    expr: Box::new(expr),
                    ty,
                    span,
                })
            }
            TokenKind::Amp => {
                // Address-of: &var
                self.advance();
                let expr = self.parse_unary()?;
                let ty = Type::Pointer(Box::new(expr.get_type()));
                Ok(Expr::Unary {
                    op: UnaryOp::AddrOf,
                    expr: Box::new(expr),
                    ty,
                    span,
                })
            }
            TokenKind::PlusPlus => {
                // Pre-increment: ++x -> (x += 1)
                self.advance();
                let expr = self.parse_unary()?;
                let ty = expr.get_type();
                Ok(Expr::Unary {
                    op: UnaryOp::PreInc,
                    expr: Box::new(expr),
                    ty,
                    span,
                })
            }
            TokenKind::MinusMinus => {
                // Pre-decrement: --x -> (x -= 1)
                self.advance();
                let expr = self.parse_unary()?;
                let ty = expr.get_type();
                Ok(Expr::Unary {
                    op: UnaryOp::PreDec,
                    expr: Box::new(expr),
                    ty,
                    span,
                })
            }
            TokenKind::Sizeof => {
                self.advance();
                // Can be sizeof(type) or sizeof expr
                if self.check(&TokenKind::LParen) && self.peek_ahead(1).map_or(false, |t| {
                    matches!(
                        t.kind,
                        TokenKind::Int | TokenKind::Char | TokenKind::Short | TokenKind::Long | TokenKind::Void
                    )
                }) {
                    self.advance(); // '('
                    let base = self.parse_base_type()?;
                    let (ty, _, _) = self.parse_declarator(base)?;
                    self.expect(&TokenKind::RParen, "')'")?;
                    Ok(Expr::Number(ty.size() as i64, Type::Long, span))
                } else {
                    let expr = self.parse_unary()?;
                    let sz = expr.get_type().size();
                    Ok(Expr::Number(sz as i64, Type::Long, span))
                }
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.match_token(&TokenKind::PlusPlus) {
                let ty = expr.get_type();
                let span = expr.span();
                expr = Expr::Unary {
                    op: UnaryOp::PostInc,
                    expr: Box::new(expr),
                    ty,
                    span,
                };
            } else if self.match_token(&TokenKind::MinusMinus) {
                let ty = expr.get_type();
                let span = expr.span();
                expr = Expr::Unary {
                    op: UnaryOp::PostDec,
                    expr: Box::new(expr),
                    ty,
                    span,
                };
            } else if self.match_token(&TokenKind::LBracket) {
                // Array subscript: a[i] == *(a + i)
                let index = self.parse_expression()?;
                self.expect(&TokenKind::RBracket, "']'")?;

                let elem_ty = match expr.get_type().base_type() {
                    Some(b) => b.clone(),
                    None => Type::Int,
                };
                let elem_size = elem_ty.size() as i64;

                let scaled_index = if elem_size > 1 {
                    Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(index.clone()),
                        right: Box::new(Expr::Number(elem_size, Type::Int, index.span())),
                        ty: Type::Long,
                        span: index.span(),
                    }
                } else {
                    index
                };

                let ptr_add = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(expr),
                    right: Box::new(scaled_index),
                    ty: Type::Pointer(Box::new(elem_ty.clone())),
                    span: SourceSpan {
                        start_line: 0,
                        start_col: 0,
                        end_line: 0,
                        end_col: 0,
                    },
                };

                expr = Expr::Unary {
                    op: UnaryOp::Deref,
                    expr: Box::new(ptr_add),
                    ty: elem_ty,
                    span: SourceSpan {
                        start_line: 0,
                        start_col: 0,
                        end_line: 0,
                        end_col: 0,
                    },
                };
            } else if self.match_token(&TokenKind::LParen) {
                // Function call: ident(args)
                let callee_name = match &expr {
                    Expr::Variable(name, _, _) => name.clone(),
                    _ => {
                        return Err(CompileError {
                            message: "Expression is not callable".to_string(),
                            line: expr.span().start_line,
                            column: expr.span().start_col,
                        })
                    }
                };

                let mut args = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_assignment()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end_span = self.expect(&TokenKind::RParen, "')'")?.span;

                let span = SourceSpan {
                    start_line: expr.span().start_line,
                    start_col: expr.span().start_col,
                    end_line: end_span.end_line,
                    end_col: end_span.end_col,
                };

                expr = Expr::Call {
                    callee: callee_name,
                    args,
                    ty: Type::Int, // default function return type
                    span,
                };
            } else if self.match_token(&TokenKind::Dot) {
                let tok = self.advance().clone();
                let (member_name, mem_span) = match tok.kind {
                    TokenKind::Ident(id) => (id, tok.span),
                    other => {
                        return Err(CompileError {
                            message: format!("Expected member name after '.', found {:?}", other),
                            line: tok.span.start_line,
                            column: tok.span.start_col,
                        })
                    }
                };
                let struct_ty = expr.get_type();
                let member = match struct_ty.get_struct_member(&member_name) {
                    Some(m) => m.clone(),
                    None => {
                        return Err(CompileError {
                            message: format!(
                                "No member named '{}' in {}",
                                member_name,
                                struct_ty.type_name()
                            ),
                            line: mem_span.start_line,
                            column: mem_span.start_col,
                        })
                    }
                };
                let span = SourceSpan {
                    start_line: expr.span().start_line,
                    start_col: expr.span().start_col,
                    end_line: mem_span.end_line,
                    end_col: mem_span.end_col,
                };
                expr = Expr::MemberAccess {
                    expr: Box::new(expr),
                    member: member_name,
                    ty: member.ty,
                    offset: member.offset,
                    span,
                };
            } else if self.match_token(&TokenKind::Arrow) {
                let tok = self.advance().clone();
                let (member_name, mem_span) = match tok.kind {
                    TokenKind::Ident(id) => (id, tok.span),
                    other => {
                        return Err(CompileError {
                            message: format!("Expected member name after '->', found {:?}", other),
                            line: tok.span.start_line,
                            column: tok.span.start_col,
                        })
                    }
                };
                let ptr_ty = expr.get_type();
                let struct_ty = match ptr_ty.base_type() {
                    Some(b) if b.is_struct() => b.clone(),
                    _ => {
                        return Err(CompileError {
                            message: format!(
                                "Cannot use '->' on non-pointer-to-struct type {}",
                                ptr_ty.type_name()
                            ),
                            line: mem_span.start_line,
                            column: mem_span.start_col,
                        })
                    }
                };
                let member = match struct_ty.get_struct_member(&member_name) {
                    Some(m) => m.clone(),
                    None => {
                        return Err(CompileError {
                            message: format!(
                                "No member named '{}' in {}",
                                member_name,
                                struct_ty.type_name()
                            ),
                            line: mem_span.start_line,
                            column: mem_span.start_col,
                        })
                    }
                };
                let span = SourceSpan {
                    start_line: expr.span().start_line,
                    start_col: expr.span().start_col,
                    end_line: mem_span.end_line,
                    end_col: mem_span.end_col,
                };
                let deref_expr = Expr::Unary {
                    op: UnaryOp::Deref,
                    expr: Box::new(expr),
                    ty: struct_ty,
                    span,
                };
                expr = Expr::MemberAccess {
                    expr: Box::new(deref_expr),
                    member: member_name,
                    ty: member.ty,
                    offset: member.offset,
                    span,
                };
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, CompileError> {
        let tok = self.advance().clone();
        match tok.kind {
            TokenKind::Number(n) => Ok(Expr::Number(n, Type::Int, tok.span)),
            TokenKind::CharLiteral(c) => Ok(Expr::Number(c as i64, Type::Char, tok.span)),
            TokenKind::StringLiteral(bytes) => {
                let id = self.next_string_id;
                self.next_string_id += 1;
                self.string_literals.push((id, bytes));
                Ok(Expr::StringLiteral(id, tok.span))
            }
            TokenKind::Ident(ref name) => {
                // Resolve variable type
                if let Some(local) = self.find_local_var(name) {
                    let mut ty = local.ty.clone();
                    // Array decays to pointer in expressions
                    if let Type::Array(inner, _) = ty {
                        ty = Type::Pointer(inner);
                    }
                    Ok(Expr::Variable(name.clone(), ty, tok.span))
                } else if let Some(global) = self.find_global_var(name) {
                    let mut ty = global.ty.clone();
                    if let Type::Array(inner, _) = ty {
                        ty = Type::Pointer(inner);
                    }
                    Ok(Expr::Variable(name.clone(), ty, tok.span))
                } else {
                    // Function name or undeclared variable (default to int)
                    Ok(Expr::Variable(name.clone(), Type::Int, tok.span))
                }
            }
            TokenKind::LParen => {
                let expr = self.parse_expression()?;
                self.expect(&TokenKind::RParen, "')'")?;
                Ok(expr)
            }
            _ => Err(CompileError {
                message: format!("Expected expression, found {:?}", tok.kind),
                line: tok.span.start_line,
                column: tok.span.start_col,
            }),
        }
    }
}
