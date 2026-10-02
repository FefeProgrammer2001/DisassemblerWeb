use crate::metadata::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructMember {
    pub name: String,
    pub ty: Type,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Void,
    Char,
    Short,
    Int,
    Long,
    Pointer(Box<Type>),
    Array(Box<Type>, usize),
    Struct {
        name: Option<String>,
        members: Vec<StructMember>,
        size: usize,
        align: usize,
    },
}

impl Type {
    pub fn size(&self) -> usize {
        match self {
            Type::Void => 0,
            Type::Char => 1,
            Type::Short => 2,
            Type::Int => 4,
            Type::Long => 8,
            Type::Pointer(_) => 8,
            Type::Array(elem_ty, count) => elem_ty.size() * count,
            Type::Struct { size, .. } => *size,
        }
    }

    pub fn alignment(&self) -> usize {
        match self {
            Type::Void => 1,
            Type::Char => 1,
            Type::Short => 2,
            Type::Int => 4,
            Type::Long => 8,
            Type::Pointer(_) => 8,
            Type::Array(elem_ty, _) => elem_ty.alignment(),
            Type::Struct { align, .. } => *align,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Type::Char | Type::Short | Type::Int | Type::Long)
    }

    pub fn is_pointer(&self) -> bool {
        matches!(self, Type::Pointer(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Type::Array(_, _))
    }

    pub fn is_struct(&self) -> bool {
        matches!(self, Type::Struct { .. })
    }

    pub fn base_type(&self) -> Option<&Type> {
        match self {
            Type::Pointer(base) | Type::Array(base, _) => Some(base),
            _ => None,
        }
    }

    pub fn get_struct_member(&self, member_name: &str) -> Option<&StructMember> {
        match self {
            Type::Struct { members, .. } => members.iter().find(|m| m.name == member_name),
            _ => None,
        }
    }

    pub fn type_name(&self) -> String {
        match self {
            Type::Void => "void".to_string(),
            Type::Char => "char".to_string(),
            Type::Short => "short".to_string(),
            Type::Int => "int".to_string(),
            Type::Long => "long".to_string(),
            Type::Pointer(inner) => format!("{}*", inner.type_name()),
            Type::Array(inner, size) => format!("{}[{}]", inner.type_name(), size),
            Type::Struct { name, .. } => name
                .as_ref()
                .map(|n| format!("struct {}", n))
                .unwrap_or_else(|| "struct <anon>".to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    LogicalAnd,
    LogicalOr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Pos,
    Not,
    BitNot,
    Deref,
    AddrOf,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(i64, Type, SourceSpan),
    StringLiteral(usize, SourceSpan), // string literal ID
    Variable(String, Type, SourceSpan),
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        ty: Type,
        span: SourceSpan,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        ty: Type,
        span: SourceSpan,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        ty: Type,
        span: SourceSpan,
    },
    CompoundAssign {
        op: BinaryOp,
        target: Box<Expr>,
        value: Box<Expr>,
        ty: Type,
        span: SourceSpan,
    },
    Call {
        callee: String,
        args: Vec<Expr>,
        ty: Type,
        span: SourceSpan,
    },
    Comma {
        left: Box<Expr>,
        right: Box<Expr>,
        ty: Type,
        span: SourceSpan,
    },
    Cast {
        expr: Box<Expr>,
        target_type: Type,
        span: SourceSpan,
    },
    MemberAccess {
        expr: Box<Expr>,
        member: String,
        ty: Type,
        offset: usize,
        span: SourceSpan,
    },
}

impl Expr {
    pub fn get_type(&self) -> Type {
        match self {
            Expr::Number(_, ty, _) => ty.clone(),
            Expr::StringLiteral(_, _) => Type::Pointer(Box::new(Type::Char)),
            Expr::Variable(_, ty, _) => ty.clone(),
            Expr::Binary { ty, .. } => ty.clone(),
            Expr::Unary { ty, .. } => ty.clone(),
            Expr::Assign { ty, .. } => ty.clone(),
            Expr::CompoundAssign { ty, .. } => ty.clone(),
            Expr::Call { ty, .. } => ty.clone(),
            Expr::Comma { ty, .. } => ty.clone(),
            Expr::Cast { target_type, .. } => target_type.clone(),
            Expr::MemberAccess { ty, .. } => ty.clone(),
        }
    }

    pub fn span(&self) -> SourceSpan {
        match self {
            Expr::Number(_, _, s) => *s,
            Expr::StringLiteral(_, s) => *s,
            Expr::Variable(_, _, s) => *s,
            Expr::Binary { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Assign { span, .. } => *span,
            Expr::CompoundAssign { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::Comma { span, .. } => *span,
            Expr::Cast { span, .. } => *span,
            Expr::MemberAccess { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VarDeclItem {
    pub name: String,
    pub ty: Type,
    pub init: Option<Expr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Block(Vec<Stmt>, SourceSpan),
    Return(Option<Expr>, SourceSpan),
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
        span: SourceSpan,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
        span: SourceSpan,
    },
    DoWhile {
        body: Box<Stmt>,
        cond: Expr,
        span: SourceSpan,
    },
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Box<Stmt>,
        span: SourceSpan,
    },
    Break(SourceSpan),
    Continue(SourceSpan),
    VarDecl(Vec<VarDeclItem>),
}

impl Stmt {
    pub fn span(&self) -> SourceSpan {
        match self {
            Stmt::Expr(e) => e.span(),
            Stmt::Block(_, s) => *s,
            Stmt::Return(_, s) => *s,
            Stmt::If { span, .. } => *span,
            Stmt::While { span, .. } => *span,
            Stmt::DoWhile { span, .. } => *span,
            Stmt::For { span, .. } => *span,
            Stmt::Break(s) => *s,
            Stmt::Continue(s) => *s,
            Stmt::VarDecl(items) => items.first().map(|i| i.span).unwrap_or(SourceSpan {
                start_line: 1,
                start_col: 1,
                end_line: 1,
                end_col: 1,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalVar {
    pub name: String,
    pub ty: Type,
    pub rbp_offset: i32,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub enum GlobalInit {
    Number(i64),
    StringLiteral(usize),
    None,
}

#[derive(Debug, Clone)]
pub struct GlobalVar {
    pub name: String,
    pub ty: Type,
    pub init_val: GlobalInit,
}

#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub ret_type: Type,
    pub params: Vec<(String, Type)>,
    pub body: Vec<Stmt>,
    pub locals: Vec<LocalVar>,
    pub stack_size: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub globals: Vec<GlobalVar>,
    pub functions: Vec<FunctionDef>,
    pub string_literals: Vec<(usize, Vec<u8>)>,
}
