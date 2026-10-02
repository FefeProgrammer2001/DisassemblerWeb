use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::compile;
use crate::metadata::{AsmLine, CompileOutput};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryAccessType {
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAccessEvent {
    pub access_type: MemoryAccessType,
    pub address: u64,
    pub size: usize,
    pub value: u64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterChange {
    pub name: String,
    pub old_value: u64,
    pub new_value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrameState {
    pub function_name: String,
    pub rbp: u64,
    pub rsp: u64,
    pub return_rip: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub current_line: usize,
    pub source_line: Option<usize>,
    pub instruction_text: String,
    pub explanation: String,
    pub register_changes: Vec<RegisterChange>,
    pub memory_events: Vec<MemoryAccessEvent>,
    pub call_stack: Vec<StackFrameState>,
    pub is_halted: bool,
    pub return_value: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CpuStateSnapshot {
    pub registers: HashMap<String, u64>,
    pub flags: HashMap<String, bool>,
    pub rip: usize,
    pub stack_memory: Vec<(u64, u8)>, // (address, byte)
    pub call_stack: Vec<StackFrameState>,
}

pub struct Stepper {
    pub compile_output: CompileOutput,
    pub lines: Vec<AsmLine>,
    pub label_map: HashMap<String, usize>, // label -> line index

    // CPU Registers
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,

    // Flags
    pub zf: bool,
    pub sf: bool,

    // Instruction Pointer (index in `self.lines`)
    pub rip: usize,

    // Memory (Sparse 64-bit address space)
    pub memory: HashMap<u64, u8>,

    // Symbol addresses
    pub symbol_addrs: HashMap<String, u64>,

    // Call stack tracking
    pub call_stack: Vec<StackFrameState>,

    // History for time travel (Step Back)
    pub history: Vec<CpuStateSnapshot>,

    pub is_halted: bool,
    pub return_value: Option<i64>,
}

impl Stepper {
    pub fn new(c_source: &str) -> Result<Self, String> {
        let output = compile(c_source);
        if !output.success {
            let err_msgs = output
                .errors
                .iter()
                .map(|e| format!("[{}:{}] {}", e.line, e.column, e.message))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!("Compilation failed: {}", err_msgs));
        }

        Self::from_compile_output(output)
    }

    pub fn from_compile_output(output: CompileOutput) -> Result<Self, String> {
        let lines = output.lines.clone();
        let mut label_map = HashMap::new();

        // Index all labels
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.text.trim();
            if trimmed.ends_with(':') {
                let lbl = trimmed[..trimmed.len() - 1].trim();
                label_map.insert(lbl.to_string(), idx);
            }
        }

        // Initialize Virtual Address Space
        // Stack base at 0x7FFF_FFFF_0000
        let stack_base = 0x7FFF_FFFF_0000u64;
        let memory = HashMap::new();
        let mut symbol_addrs = HashMap::new();

        // Allocate globals starting at 0x1000_0000
        let mut next_global_addr = 0x1000_0000u64;
        for g in &output.globals {
            let addr = next_global_addr;
            symbol_addrs.insert(g.name.clone(), addr);
            next_global_addr += (g.size.max(4) as u64 + 7) & !7; // 8-byte aligned
        }

        // Locate entry point (default "main", or first function)
        let entry_label = if label_map.contains_key("main") {
            "main".to_string()
        } else if let Some(first_func) = output.functions.first() {
            first_func.name.clone()
        } else {
            return Err("No function found to execute".to_string());
        };

        let start_rip = match label_map.get(&entry_label) {
            Some(&idx) => idx,
            None => return Err(format!("Entry label '{}' not found", entry_label)),
        };

        let stepper = Self {
            compile_output: output,
            lines,
            label_map,
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: stack_base,
            rsp: stack_base,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            zf: false,
            sf: false,
            rip: start_rip,
            memory,
            symbol_addrs,
            call_stack: vec![StackFrameState {
                function_name: entry_label.to_string(),
                rbp: stack_base,
                rsp: stack_base,
                return_rip: usize::MAX, // marks program termination on ret
            }],
            history: Vec::new(),
            is_halted: false,
            return_value: None,
        };

        Ok(stepper)
    }

    // Memory reading
    pub fn read_u8(&self, addr: u64) -> u8 {
        *self.memory.get(&addr).unwrap_or(&0)
    }

    pub fn read_u16(&self, addr: u64) -> u16 {
        let b0 = self.read_u8(addr) as u16;
        let b1 = self.read_u8(addr + 1) as u16;
        b0 | (b1 << 8)
    }

    pub fn read_u32(&self, addr: u64) -> u32 {
        let b0 = self.read_u8(addr) as u32;
        let b1 = self.read_u8(addr + 1) as u32;
        let b2 = self.read_u8(addr + 2) as u32;
        let b3 = self.read_u8(addr + 3) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    pub fn read_u64(&self, addr: u64) -> u64 {
        let mut bytes = [0u8; 8];
        for i in 0..8 {
            bytes[i] = self.read_u8(addr + i as u64);
        }
        u64::from_le_bytes(bytes)
    }

    // Memory writing
    pub fn write_u8(&mut self, addr: u64, val: u8) {
        self.memory.insert(addr, val);
    }

    pub fn write_u16(&mut self, addr: u64, val: u16) {
        let bytes = val.to_le_bytes();
        self.write_u8(addr, bytes[0]);
        self.write_u8(addr + 1, bytes[1]);
    }

    pub fn write_u32(&mut self, addr: u64, val: u32) {
        let bytes = val.to_le_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            self.write_u8(addr + i as u64, b);
        }
    }

    pub fn write_u64(&mut self, addr: u64, val: u64) {
        let bytes = val.to_le_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            self.write_u8(addr + i as u64, b);
        }
    }

    pub fn get_reg(&self, name: &str) -> u64 {
        match name.to_lowercase().as_str() {
            "rax" => self.rax,
            "eax" => self.rax & 0xFFFF_FFFF,
            "ax" => self.rax & 0xFFFF,
            "al" => self.rax & 0xFF,
            "rbx" => self.rbx,
            "ebx" => self.rbx & 0xFFFF_FFFF,
            "bx" => self.rbx & 0xFFFF,
            "bl" => self.rbx & 0xFF,
            "rcx" => self.rcx,
            "ecx" => self.rcx & 0xFFFF_FFFF,
            "cx" => self.rcx & 0xFFFF,
            "cl" => self.rcx & 0xFF,
            "rdx" => self.rdx,
            "edx" => self.rdx & 0xFFFF_FFFF,
            "dx" => self.rdx & 0xFFFF,
            "dl" => self.rdx & 0xFF,
            "rsi" => self.rsi,
            "esi" => self.rsi & 0xFFFF_FFFF,
            "si" => self.rsi & 0xFFFF,
            "sil" => self.rsi & 0xFF,
            "rdi" => self.rdi,
            "edi" => self.rdi & 0xFFFF_FFFF,
            "di" => self.rdi & 0xFFFF,
            "dil" => self.rdi & 0xFF,
            "rbp" => self.rbp,
            "ebp" => self.rbp & 0xFFFF_FFFF,
            "rsp" => self.rsp,
            "esp" => self.rsp & 0xFFFF_FFFF,
            "r8" => self.r8,
            "r8d" => self.r8 & 0xFFFF_FFFF,
            "r8w" => self.r8 & 0xFFFF,
            "r8b" => self.r8 & 0xFF,
            "r9" => self.r9,
            "r9d" => self.r9 & 0xFFFF_FFFF,
            "r9w" => self.r9 & 0xFFFF,
            "r9b" => self.r9 & 0xFF,
            _ => 0,
        }
    }

    pub fn set_reg(&mut self, name: &str, val: u64) {
        match name.to_lowercase().as_str() {
            "rax" => self.rax = val,
            "eax" => self.rax = val & 0xFFFF_FFFF,
            "ax" => self.rax = (self.rax & !0xFFFF) | (val & 0xFFFF),
            "al" => self.rax = (self.rax & !0xFF) | (val & 0xFF),
            "rbx" => self.rbx = val,
            "ebx" => self.rbx = val & 0xFFFF_FFFF,
            "bx" => self.rbx = (self.rbx & !0xFFFF) | (val & 0xFFFF),
            "bl" => self.rbx = (self.rbx & !0xFF) | (val & 0xFF),
            "rcx" => self.rcx = val,
            "ecx" => self.rcx = val & 0xFFFF_FFFF,
            "cx" => self.rcx = (self.rcx & !0xFFFF) | (val & 0xFFFF),
            "cl" => self.rcx = (self.rcx & !0xFF) | (val & 0xFF),
            "rdx" => self.rdx = val,
            "edx" => self.rdx = val & 0xFFFF_FFFF,
            "dx" => self.rdx = (self.rdx & !0xFFFF) | (val & 0xFFFF),
            "dl" => self.rdx = (self.rdx & !0xFF) | (val & 0xFF),
            "rsi" => self.rsi = val,
            "esi" => self.rsi = val & 0xFFFF_FFFF,
            "si" => self.rsi = (self.rsi & !0xFFFF) | (val & 0xFFFF),
            "sil" => self.rsi = (self.rsi & !0xFF) | (val & 0xFF),
            "rdi" => self.rdi = val,
            "edi" => self.rdi = val & 0xFFFF_FFFF,
            "di" => self.rdi = (self.rdi & !0xFFFF) | (val & 0xFFFF),
            "dil" => self.rdi = (self.rdi & !0xFF) | (val & 0xFF),
            "rbp" => self.rbp = val,
            "ebp" => self.rbp = val & 0xFFFF_FFFF,
            "rsp" => self.rsp = val,
            "esp" => self.rsp = val & 0xFFFF_FFFF,
            "r8" => self.r8 = val,
            "r8d" => self.r8 = val & 0xFFFF_FFFF,
            "r8w" => self.r8 = (self.r8 & !0xFFFF) | (val & 0xFFFF),
            "r8b" => self.r8 = (self.r8 & !0xFF) | (val & 0xFF),
            "r9" => self.r9 = val,
            "r9d" => self.r9 = val & 0xFFFF_FFFF,
            "r9w" => self.r9 = (self.r9 & !0xFFFF) | (val & 0xFFFF),
            "r9b" => self.r9 = (self.r9 & !0xFF) | (val & 0xFF),
            _ => {}
        }
    }

    fn parse_effective_address(&self, expr_str: &str) -> u64 {
        // Examples: [rbp-4], [rbp+16], [rax], [rdi], [rsp+8], [var_name]
        let inner = expr_str.trim().trim_start_matches('[').trim_end_matches(']').trim();

        if let Some(pos) = inner.find('+') {
            let base_name = &inner[..pos].trim();
            let offset_str = &inner[pos + 1..].trim();
            let base = self.get_reg(base_name);
            let offset = offset_str.parse::<u64>().unwrap_or(0);
            base + offset
        } else if let Some(pos) = inner.find('-') {
            let base_name = &inner[..pos].trim();
            let offset_str = &inner[pos + 1..].trim();
            let base = self.get_reg(base_name);
            let offset = offset_str.parse::<u64>().unwrap_or(0);
            base.wrapping_sub(offset)
        } else if let Some(&addr) = self.symbol_addrs.get(inner) {
            addr
        } else {
            self.get_reg(inner)
        }
    }

    fn parse_operand_value(&self, op_str: &str) -> u64 {
        let s = op_str.trim();

        // Hex or Dec Number
        if s.starts_with("0x") || s.starts_with("0X") {
            return u64::from_str_radix(&s[2..], 16).unwrap_or(0);
        }
        if let Ok(n) = s.parse::<i64>() {
            return n as u64;
        }

        // Memory operand: BYTE/WORD/DWORD/QWORD PTR [...]
        if s.contains('[') && s.contains(']') {
            let addr = self.parse_effective_address(s[s.find('[').unwrap()..s.rfind(']').unwrap() + 1].trim());
            if s.starts_with("BYTE PTR") {
                return self.read_u8(addr) as u64;
            } else if s.starts_with("WORD PTR") {
                return self.read_u16(addr) as u64;
            } else if s.starts_with("DWORD PTR") {
                return self.read_u32(addr) as u64;
            } else {
                return self.read_u64(addr);
            }
        }

        // Register
        self.get_reg(s)
    }

    /// Takes a snapshot of current CPU & memory state
    fn take_snapshot(&self) -> CpuStateSnapshot {
        let mut registers = HashMap::new();
        for &name in &["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "r8", "r9"] {
            registers.insert(name.to_string(), self.get_reg(name));
        }

        let mut flags = HashMap::new();
        flags.insert("zf".to_string(), self.zf);
        flags.insert("sf".to_string(), self.sf);

        let stack_memory = self
            .memory
            .iter()
            .filter(|(&addr, _)| addr >= self.rsp && addr <= self.rbp + 64)
            .map(|(&addr, &b)| (addr, b))
            .collect();

        CpuStateSnapshot {
            registers,
            flags,
            rip: self.rip,
            stack_memory,
            call_stack: self.call_stack.clone(),
        }
    }

    /// Single-steps one instruction
    pub fn step(&mut self) -> Result<StepResult, String> {
        if self.is_halted {
            return Ok(StepResult {
                current_line: self.rip,
                source_line: None,
                instruction_text: "HALTED".to_string(),
                explanation: "Execution finished".to_string(),
                register_changes: Vec::new(),
                memory_events: Vec::new(),
                call_stack: self.call_stack.clone(),
                is_halted: true,
                return_value: self.return_value,
            });
        }

        // Skip non-instruction lines (directives, comments, labels)
        while self.rip < self.lines.len() {
            let line = &self.lines[self.rip];
            let trimmed = line.text.trim();

            if trimmed.is_empty()
                || trimmed.starts_with('.')
                || trimmed.starts_with('#')
            {
                self.rip += 1;
                continue;
            }

            if trimmed.ends_with(':') {
                // Label
                self.rip += 1;
                continue;
            }

            break;
        }

        if self.rip >= self.lines.len() {
            self.is_halted = true;
            return Ok(StepResult {
                current_line: self.rip,
                source_line: None,
                instruction_text: "EOF".to_string(),
                explanation: "Reached end of assembly".to_string(),
                register_changes: Vec::new(),
                memory_events: Vec::new(),
                call_stack: self.call_stack.clone(),
                is_halted: true,
                return_value: Some(self.rax as i64),
            });
        }

        // Save snapshot for step_back
        self.history.push(self.take_snapshot());

        let line = self.lines[self.rip].clone();
        let inst_str = line.text.trim();
        let current_line_idx = self.rip;

        // Record initial state of registers to detect changes
        let old_regs = [
            ("rax", self.rax),
            ("rbx", self.rbx),
            ("rcx", self.rcx),
            ("rdx", self.rdx),
            ("rsi", self.rsi),
            ("rdi", self.rdi),
            ("rbp", self.rbp),
            ("rsp", self.rsp),
            ("r8", self.r8),
            ("r9", self.r9),
        ];

        let mut memory_events = Vec::new();
        let mut explanation = line.comment.clone().unwrap_or_else(|| inst_str.to_string());
        let mut next_rip = self.rip + 1;

        // Parse opcode and operands
        let parts: Vec<&str> = inst_str.splitn(2, ' ').collect();
        let opcode = parts[0].trim().to_lowercase();
        let operands_str = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match opcode.as_str() {
            "push" => {
                let val = self.parse_operand_value(operands_str);
                self.rsp -= 8;
                self.write_u64(self.rsp, val);
                memory_events.push(MemoryAccessEvent {
                    access_type: MemoryAccessType::Write,
                    address: self.rsp,
                    size: 8,
                    value: val,
                    description: format!("Push value 0x{:x} to stack", val),
                });
                explanation = format!("Pushed 0x{:x} to stack at 0x{:x}", val, self.rsp);
            }
            "pop" => {
                let reg_name = operands_str.trim();
                let val = self.read_u64(self.rsp);
                self.set_reg(reg_name, val);
                memory_events.push(MemoryAccessEvent {
                    access_type: MemoryAccessType::Read,
                    address: self.rsp,
                    size: 8,
                    value: val,
                    description: format!("Pop value 0x{:x} from stack into {}", val, reg_name.to_uppercase()),
                });
                self.rsp += 8;
                explanation = format!("Popped 0x{:x} from stack into {}", val, reg_name.to_uppercase());
            }
            "mov" | "movsx" | "movzx" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let src = ops[1].trim();

                    if dest.contains('[') {
                        // Store to memory: mov DWORD PTR [rbp-4], eax
                        let addr = self.parse_effective_address(dest[dest.find('[').unwrap()..dest.rfind(']').unwrap() + 1].trim());
                        let val = self.parse_operand_value(src);
                        let size = if dest.starts_with("BYTE PTR") {
                            self.write_u8(addr, val as u8);
                            1
                        } else if dest.starts_with("WORD PTR") {
                            self.write_u16(addr, val as u16);
                            2
                        } else if dest.starts_with("DWORD PTR") {
                            self.write_u32(addr, val as u32);
                            4
                        } else {
                            self.write_u64(addr, val);
                            8
                        };
                        memory_events.push(MemoryAccessEvent {
                            access_type: MemoryAccessType::Write,
                            address: addr,
                            size,
                            value: val,
                            description: format!("Write {} bytes (0x{:x}) to [0x{:x}]", size, val, addr),
                        });
                        explanation = format!("Stored 0x{:x} to memory [0x{:x}]", val, addr);
                    } else {
                        // Load into register: mov eax, DWORD PTR [rax] OR mov rax, 10
                        let val = self.parse_operand_value(src);
                        if src.contains('[') {
                            let addr = self.parse_effective_address(src[src.find('[').unwrap()..src.rfind(']').unwrap() + 1].trim());
                            let size = if src.starts_with("BYTE PTR") { 1 } else if src.starts_with("WORD PTR") { 2 } else if src.starts_with("DWORD PTR") { 4 } else { 8 };
                            memory_events.push(MemoryAccessEvent {
                                access_type: MemoryAccessType::Read,
                                address: addr,
                                size,
                                value: val,
                                description: format!("Read {} bytes from [0x{:x}]", size, addr),
                            });
                        }
                        self.set_reg(dest, val);
                        explanation = format!("Set {} = 0x{:x} ({})", dest.to_uppercase(), val, val as i64);
                    }
                }
            }
            "lea" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let src = ops[1].trim();
                    let addr = self.parse_effective_address(src);
                    self.set_reg(dest, addr);
                    explanation = format!("Calculated effective address 0x{:x} into {}", addr, dest.to_uppercase());
                }
            }
            "add" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let src = ops[1].trim();
                    let a = self.get_reg(dest);
                    let b = self.parse_operand_value(src);
                    let res = a.wrapping_add(b);
                    self.set_reg(dest, res);
                    self.zf = res == 0;
                    self.sf = (res as i64) < 0;
                    explanation = format!("Added {} to {}, result = {}", b, a, res);
                }
            }
            "sub" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let src = ops[1].trim();
                    let a = self.get_reg(dest);
                    let b = self.parse_operand_value(src);
                    let res = a.wrapping_sub(b);
                    self.set_reg(dest, res);
                    self.zf = res == 0;
                    self.sf = (res as i64) < 0;
                    explanation = format!("Subtracted {} from {}, result = {}", b, a, res);
                }
            }
            "imul" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let src = ops[1].trim();
                    let a = self.get_reg(dest) as i64;
                    let b = self.parse_operand_value(src) as i64;
                    let res = a.wrapping_mul(b);
                    self.set_reg(dest, res as u64);
                    self.zf = res == 0;
                    self.sf = res < 0;
                    explanation = format!("Multiplied {} by {}, result = {}", a, b, res);
                }
            }
            "cqo" => {
                let a = self.rax as i64;
                self.rdx = if a < 0 { u64::MAX } else { 0 };
                explanation = "Sign-extended RAX into RDX:RAX".to_string();
            }
            "idiv" => {
                let divisor = self.parse_operand_value(operands_str) as i64;
                if divisor != 0 {
                    let dividend = self.rax as i64;
                    let quot = dividend / divisor;
                    let rem = dividend % divisor;
                    self.rax = quot as u64;
                    self.rdx = rem as u64;
                    explanation = format!("Divided {} by {}, quot = {}, rem = {}", dividend, divisor, quot, rem);
                }
            }
            "and" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let src = ops[1].trim();
                    let res = self.get_reg(dest) & self.parse_operand_value(src);
                    self.set_reg(dest, res);
                    self.zf = res == 0;
                    self.sf = (res as i64) < 0;
                }
            }
            "or" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let src = ops[1].trim();
                    let res = self.get_reg(dest) | self.parse_operand_value(src);
                    self.set_reg(dest, res);
                    self.zf = res == 0;
                    self.sf = (res as i64) < 0;
                }
            }
            "xor" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let src = ops[1].trim();
                    let res = self.get_reg(dest) ^ self.parse_operand_value(src);
                    self.set_reg(dest, res);
                    self.zf = res == 0;
                    self.sf = (res as i64) < 0;
                }
            }
            "shl" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let count = (self.rcx & 0x3F) as u32;
                    let res = self.get_reg(dest) << count;
                    self.set_reg(dest, res);
                    self.zf = res == 0;
                }
            }
            "sar" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let dest = ops[0].trim();
                    let count = (self.rcx & 0x3F) as u32;
                    let res = ((self.get_reg(dest) as i64) >> count) as u64;
                    self.set_reg(dest, res);
                    self.zf = res == 0;
                }
            }
            "neg" => {
                let val = (self.get_reg(operands_str) as i64).wrapping_neg() as u64;
                self.set_reg(operands_str, val);
                self.zf = val == 0;
                self.sf = (val as i64) < 0;
            }
            "not" => {
                let val = !self.get_reg(operands_str);
                self.set_reg(operands_str, val);
            }
            "cmp" => {
                let ops: Vec<&str> = operands_str.splitn(2, ',').collect();
                if ops.len() == 2 {
                    let a = self.parse_operand_value(ops[0]) as i64;
                    let b = self.parse_operand_value(ops[1]) as i64;
                    self.zf = a == b;
                    self.sf = a < b;
                    explanation = format!("Compared {} with {} (ZF={}, SF={})", a, b, self.zf, self.sf);
                }
            }
            "sete" => {
                let val = if self.zf { 1 } else { 0 };
                self.set_reg(operands_str, val);
            }
            "setne" => {
                let val = if !self.zf { 1 } else { 0 };
                self.set_reg(operands_str, val);
            }
            "setl" => {
                let val = if self.sf { 1 } else { 0 };
                self.set_reg(operands_str, val);
            }
            "setle" => {
                let val = if self.sf || self.zf { 1 } else { 0 };
                self.set_reg(operands_str, val);
            }
            "setg" => {
                let val = if !self.sf && !self.zf { 1 } else { 0 };
                self.set_reg(operands_str, val);
            }
            "setge" => {
                let val = if !self.sf || self.zf { 1 } else { 0 };
                self.set_reg(operands_str, val);
            }
            "jmp" => {
                let target = operands_str.trim();
                if let Some(&target_rip) = self.label_map.get(target) {
                    next_rip = target_rip;
                    explanation = format!("Unconditional jump to {}", target);
                }
            }
            "je" => {
                let target = operands_str.trim();
                if self.zf {
                    if let Some(&target_rip) = self.label_map.get(target) {
                        next_rip = target_rip;
                        explanation = format!("Condition met (ZF=1), jumped to {}", target);
                    }
                } else {
                    explanation = "Condition not met (ZF=0), branch not taken".to_string();
                }
            }
            "jne" => {
                let target = operands_str.trim();
                if !self.zf {
                    if let Some(&target_rip) = self.label_map.get(target) {
                        next_rip = target_rip;
                        explanation = format!("Condition met (ZF=0), jumped to {}", target);
                    }
                } else {
                    explanation = "Condition not met (ZF=1), branch not taken".to_string();
                }
            }
            "call" => {
                let target = operands_str.trim();
                if let Some(&target_rip) = self.label_map.get(target) {
                    // Push call stack frame
                    self.call_stack.push(StackFrameState {
                        function_name: target.to_string(),
                        rbp: self.rbp,
                        rsp: self.rsp,
                        return_rip: self.rip + 1,
                    });
                    next_rip = target_rip;
                    explanation = format!("Called function '{}', pushed stack frame", target);
                }
            }
            "ret" => {
                if let Some(frame) = self.call_stack.pop() {
                    if frame.return_rip == usize::MAX {
                        // Returned from main / entry function!
                        self.is_halted = true;
                        self.return_value = Some(self.rax as i64);
                        explanation = format!("Program returned {}", self.rax as i64);
                    } else {
                        next_rip = frame.return_rip;
                        explanation = format!("Returned from function to caller, RAX = {}", self.rax as i64);
                    }
                } else {
                    self.is_halted = true;
                    self.return_value = Some(self.rax as i64);
                }
            }
            _ => {
                // Ignore unknown or unhandled directive
            }
        }

        self.rip = next_rip;

        // Collect register changes
        let mut register_changes = Vec::new();
        for &(name, old_val) in &old_regs {
            let new_val = self.get_reg(name);
            if new_val != old_val {
                register_changes.push(RegisterChange {
                    name: name.to_uppercase(),
                    old_value: old_val,
                    new_value: new_val,
                });
            }
        }

        Ok(StepResult {
            current_line: current_line_idx + 1,
            source_line: line.source_line,
            instruction_text: inst_str.to_string(),
            explanation,
            register_changes,
            memory_events,
            call_stack: self.call_stack.clone(),
            is_halted: self.is_halted,
            return_value: self.return_value,
        })
    }

    /// Steps backward one instruction by restoring previous snapshot
    pub fn step_back(&mut self) -> Result<bool, String> {
        if let Some(snapshot) = self.history.pop() {
            for (name, val) in snapshot.registers {
                self.set_reg(&name, val);
            }
            if let Some(&zf) = snapshot.flags.get("zf") {
                self.zf = zf;
            }
            if let Some(&sf) = snapshot.flags.get("sf") {
                self.sf = sf;
            }
            self.rip = snapshot.rip;
            for (addr, byte) in snapshot.stack_memory {
                self.memory.insert(addr, byte);
            }
            self.call_stack = snapshot.call_stack;
            self.is_halted = false;
            self.return_value = None;
            Ok(true)
        } else {
            Ok(false) // At start of program, cannot step back further
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stepper_basic_execution() {
        let code = r#"
        int main() {
            int a = 10;
            int b = 20;
            return a + b;
        }
        "#;
        let mut stepper = Stepper::new(code).expect("Failed to initialize stepper");
        let mut steps = 0;
        let mut final_ret = None;

        while steps < 100 {
            let res = stepper.step().expect("Step failed");
            if res.is_halted {
                final_ret = res.return_value;
                break;
            }
            steps += 1;
        }

        assert_eq!(final_ret, Some(30));
    }

    #[test]
    fn test_stepper_loops() {
        let code = r#"
        int main() {
            int sum = 0;
            for (int i = 1; i <= 4; i++) {
                sum += i;
            }
            return sum;
        }
        "#;
        let mut stepper = Stepper::new(code).expect("Failed to initialize stepper");
        let mut steps = 0;
        let mut final_ret = None;

        while steps < 500 {
            let res = stepper.step().expect("Step failed");
            if res.is_halted {
                final_ret = res.return_value;
                break;
            }
            steps += 1;
        }

        assert_eq!(final_ret, Some(10)); // 1 + 2 + 3 + 4 = 10
    }

    #[test]
    fn test_stepper_function_calls_and_call_stack() {
        let code = r#"
        int add(int a, int b) {
            return a + b;
        }

        int main() {
            return add(15, 27);
        }
        "#;
        let mut stepper = Stepper::new(code).expect("Failed to initialize stepper");
        let mut steps = 0;
        let mut saw_add_frame = false;
        let mut final_ret = None;

        while steps < 500 {
            let res = stepper.step().expect("Step failed");
            if res.call_stack.iter().any(|f| f.function_name == "add") {
                saw_add_frame = true;
            }
            if res.is_halted {
                final_ret = res.return_value;
                break;
            }
            steps += 1;
        }

        assert!(saw_add_frame, "Should have observed 'add' function frame on call stack");
        assert_eq!(final_ret, Some(42));
    }

    #[test]
    fn test_stepper_time_travel_step_back() {
        let code = r#"
        int main() {
            int x = 5;
            x += 10;
            return x;
        }
        "#;
        let mut stepper = Stepper::new(code).expect("Failed to initialize stepper");
        
        // Step forward 5 times
        for _ in 0..5 {
            let _ = stepper.step();
        }

        let saved_rip = stepper.rip;
        assert!(stepper.history.len() >= 5);

        // Step back
        let ok = stepper.step_back().expect("Step back failed");
        assert!(ok);
        assert!(stepper.rip < saved_rip || stepper.history.len() < 5);
    }
}
