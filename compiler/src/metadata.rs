use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsmInstructionKind {
    Directive,
    Label,
    Prologue,
    Epilogue,
    Alu,
    MemoryRead,
    MemoryWrite,
    StackOp,
    Branch,
    Call,
    Return,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsmLine {
    pub line_number: usize,
    pub text: String,
    pub source_line: Option<usize>,
    pub source_span: Option<SourceSpan>,
    pub comment: Option<String>,
    pub kind: AsmInstructionKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalVarMeta {
    pub name: String,
    pub type_name: String,
    pub size: usize,
    pub rbp_offset: i32, // e.g. -4 means [rbp - 4]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamMeta {
    pub name: String,
    pub type_name: String,
    pub size: usize,
    pub register_or_stack: String, // e.g. "EDI", "RSI", or "[rbp + 16]"
    pub rbp_offset: i32,           // local stack slot where param is saved
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMeta {
    pub name: String,
    pub return_type: String,
    pub stack_frame_size: usize,
    pub params: Vec<ParamMeta>,
    pub locals: Vec<LocalVarMeta>,
    pub asm_start_line: usize,
    pub asm_end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMeta {
    pub name: String,
    pub type_name: String,
    pub size: usize,
    pub has_initializer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileOutput {
    pub success: bool,
    pub assembly: String,
    pub lines: Vec<AsmLine>,
    pub functions: Vec<FunctionMeta>,
    pub globals: Vec<GlobalMeta>,
    pub errors: Vec<CompileError>,
}

impl CompileOutput {
    pub fn error(message: String, line: usize, column: usize) -> Self {
        Self {
            success: false,
            assembly: String::new(),
            lines: Vec::new(),
            functions: Vec::new(),
            globals: Vec::new(),
            errors: vec![CompileError {
                message,
                line,
                column,
            }],
        }
    }
}
