//! C e C++: compilação com clang/clang++ (x86_64 nativo ou AArch64 cross),
//! desmontagem com llvm-objdump e trace instrução a instrução via GDB/MI
//! (qemu-aarch64 -g para ARM64). O Rust (rust.rs) reaproveita o parser e o trace.

use crate::config;
use crate::mi::{parse_num, Gdb};
use crate::session::InputCtl;
use crate::util::*;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const COMPILE_TIMEOUT: Duration = Duration::from_secs(60);
const TRACE_SECS: u64 = 120;
const MAX_STACK: u64 = 4096;
const POINTEE_BYTES: u64 = 64;
const MAX_FRAMES: usize = 64;
const OPT_LEVELS: [&str; 5] = ["-O0", "-O1", "-O2", "-O3", "-Os"];

pub struct Lang {
    pub id: &'static str,
    pub src: &'static str,
    nobuf: &'static str,
    std: &'static str,
    cxx: bool,
}

pub const C: Lang = Lang { id: "c", src: "prog.c", nobuf: "nobuf.c", std: "-std=gnu17", cxx: false };
pub const CPP: Lang = Lang { id: "cpp", src: "prog.cpp", nobuf: "nobuf.cpp", std: "-std=gnu++20", cxx: true };

// Ligado junto ao programa: desativa o buffer do stdout para que a saída
// apareça exatamente no passo em que foi produzida.
const NOBUF: &str = r#"#include <stdio.h>
__attribute__((constructor)) static void asmviz_nobuf(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);
}
"#;

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

macro_rules! rx {
    ($pat:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        re(&R, $pat)
    }};
}

#[derive(Clone)]
pub(crate) struct Func {
    pub name: String,
    pub sym: String,
    pub start: u64,
    pub end: u64,
}

// ---------------------------------------------------------------------------
// Pós-processamento das saídas do compilador
// ---------------------------------------------------------------------------

/// Saída do clang -S: associa cada linha à linha do fonte via `.loc` e
/// descobre quais funções pertencem ao arquivo do usuário (o primeiro `.loc`
/// de cada função aponta para o fonte, e não para um header).
pub(crate) fn parse_s(text: &str, src: &str) -> (Vec<Value>, Vec<String>) {
    let fn_names: HashSet<&str> = rx!(r"(?m)^\s*\.type\s+([^\s,]+),\s*[@%]function")
        .captures_iter(text)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();
    let quoted = format!("\"{src}\"");
    let mut files: HashMap<String, bool> = HashMap::new();
    let mut out = Vec::new();
    let mut user = Vec::new();
    let mut cur: Option<u64> = None;
    let mut pending: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        let s = line.trim();
        if s.is_empty() {
            continue;
        }
        if rx!(r"^\.section\s+\.debug_").is_match(s) {
            break; // o restante é informação de depuração DWARF
        }
        if let Some(c) = rx!(r"^\.file\s+(\d+)\s+(.*)").captures(s) {
            files.insert(c[1].to_string(), c[2].contains(&quoted));
            continue;
        }
        if let Some(c) = rx!(r"^\.loc\s+(\d+)\s+(\d+)").captures(s) {
            let mine = files.get(&c[1]).copied().unwrap_or(false);
            cur = if mine { c[2].parse().ok() } else { None };
            if let Some(f) = pending.take() {
                if mine {
                    user.push(f);
                }
            }
            continue;
        }
        if s.starts_with(".cfi_") {
            continue;
        }
        let (kind, cline) = if s.starts_with('#') || s.starts_with("//") || s.starts_with(';') {
            ("comment", None)
        } else if rx!(r"^[A-Za-z_.$][\w.$]*:").is_match(line) {
            let name = s.split(':').next().unwrap_or("");
            if fn_names.contains(name) {
                pending = Some(name.to_string());
            }
            if name.starts_with(".Lfunc_end") {
                cur = None;
            }
            ("label", None)
        } else if s.starts_with('.') {
            let data = rx!(r"^\.(asciz|ascii|string|byte|short|hword|word|long|int|quad|xword|zero|space|float|double)\b");
            (if data.is_match(s) { "data" } else { "dir" }, None)
        } else {
            ("insn", cur)
        };
        out.push(json!({"t": line, "kind": kind, "cline": cline}));
    }
    (out, user)
}

pub(crate) fn parse_nm(text: &str, names: &HashSet<String>) -> Vec<Func> {
    let mut funcs: Vec<Func> = text
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            if p.len() == 4 && names.contains(p[3]) && "tTwW".contains(p[2]) {
                let start = u64::from_str_radix(p[0], 16).ok()?;
                let size = u64::from_str_radix(p[1], 16).ok()?;
                Some(Func { name: p[3].to_string(), sym: p[3].to_string(), start, end: start + size })
            } else {
                None
            }
        })
        .collect();
    funcs.sort_by_key(|f| f.start);
    funcs
}

pub(crate) fn parse_objdump(text: &str, src: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut cur: Option<u64> = None;
    for line in text.lines() {
        if let Some(c) = rx!(r"^([0-9a-f]+) <(.+)>:$").captures(line) {
            cur = None;
            let addr = u64::from_str_radix(&c[1], 16).unwrap_or(0);
            out.push(json!({"kind": "label", "t": format!("{}:", &c[2]), "addr": addr, "cline": null}));
            continue;
        }
        if line.starts_with(';') {
            if let Some(c) = rx!(r"^;\s*(.+?):(\d+)(?::\d+)?\s*$").captures(line) {
                cur = if c[1].ends_with(src) { c[2].parse().ok() } else { None };
            }
            continue;
        }
        if let Some(c) = rx!(r"^\s*([0-9a-f]+):\s*(\S.*)$").captures(line) {
            let addr = u64::from_str_radix(&c[1], 16).unwrap_or(0);
            let t = rx!(r"\s+").replace_all(c[2].trim(), " ").into_owned();
            out.push(json!({"kind": "insn", "t": t, "addr": addr, "cline": cur}));
        }
    }
    out
}

pub(crate) fn parse_sections(text: &str) -> Vec<Value> {
    const KEEP: [&str; 9] =
        [".text", ".rodata", ".data", ".bss", ".tdata", ".tbss", ".data.rel.ro", ".init_array", ".fini_array"];
    text.lines()
        .filter_map(|l| {
            let c = rx!(r"^\s*\d+\s+(\S+)\s+([0-9a-f]+)\s+([0-9a-f]+)").captures(l)?;
            KEEP.contains(&&c[1]).then(|| {
                json!({
                    "name": &c[1],
                    "size": u64::from_str_radix(&c[2], 16).unwrap_or(0),
                    "addr": u64::from_str_radix(&c[3], 16).unwrap_or(0),
                })
            })
        })
        .collect()
}

/// Demangle de nomes C++ (`_Z...`) e Rust v0 (`_R...`) usando llvm-cxxfilt em lote.
pub(crate) fn demangler(texts: &[&str], cwd: &Path) -> HashMap<String, String> {
    let mut syms: Vec<String> = texts
        .iter()
        .flat_map(|t| rx!(r"_[ZR][\w$.]+").find_iter(t).map(|m| m.as_str().to_string()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    syms.truncate(4000);
    let mut map = HashMap::new();
    if syms.is_empty() {
        return map;
    }
    let mut cmd = vec![config().cxxfilt.clone()];
    cmd.extend(syms.iter().cloned());
    let out = run(&cmd, cwd, Duration::from_secs(20));
    for (s, d) in syms.iter().zip(out.stdout.lines()) {
        if d != s {
            map.insert(s.clone(), d.to_string());
        }
    }
    map
}

pub(crate) fn demangle(s: &str, map: &HashMap<String, String>) -> String {
    if map.is_empty() {
        return s.to_string();
    }
    rx!(r"_[ZR][\w$.]+")
        .replace_all(s, |c: &regex::Captures| map.get(&c[0]).cloned().unwrap_or_else(|| c[0].to_string()))
        .into_owned()
}

// ---------------------------------------------------------------------------
// Compilação
// ---------------------------------------------------------------------------
/// `ctl` existe quando o trace foi pedido (entrada interativa, ver session.rs).
pub fn build(req: &Value, lang: &Lang, ctl: Option<&mut InputCtl>) -> Value {
    let cfg = config();
    let arch = req["arch"].as_str().unwrap_or("x86_64");
    let (triple, extra) = match arch {
        "x86_64" => ("x86_64-linux-gnu", vec![]),
        // Cross compiling: alvo AArch64 + sysroot com headers/libs da glibc ARM64.
        "arm64" => ("aarch64-linux-gnu", vec![format!("--sysroot={}", cfg.sysroot)]),
        _ => return json!({"ok": false, "diagnostics": "arquitetura inválida"}),
    };
    let intel = arch == "x86_64" && req["syntax"] == "intel";
    let opt = req["opt"].as_str().filter(|o| OPT_LEVELS.contains(o)).unwrap_or("-O0");
    let max_steps = req["maxSteps"].as_u64().unwrap_or(3000).clamp(1, 20000) as usize;

    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => return json!({"ok": false, "diagnostics": format!("erro ao criar diretório temporário: {e}")}),
    };
    let w = dir.path();
    let code = req["code"].as_str().unwrap_or("");
    let writes = [
        fs::write(w.join(lang.src), code),
        fs::write(w.join(lang.nobuf), NOBUF),
    ];
    if let Some(Err(e)) = writes.into_iter().find(|r| r.is_err()) {
        return json!({"ok": false, "diagnostics": format!("erro ao gravar arquivos: {e}")});
    }

    let compiler = if lang.cxx { &cfg.clangxx } else { &cfg.clang };
    let mut base = vec![compiler.clone(), format!("--target={triple}")];
    base.extend(extra);
    base.extend(sv(&[opt, "-g", lang.std, "-fno-omit-frame-pointer", "-fno-stack-protector"]));

    let mut res = Map::new();
    res.insert("lang".into(), json!(lang.id));
    res.insert("arch".into(), json!(arch));
    let mut commands: Vec<String> = Vec::new();
    let mut diag = String::new();
    let finish = |mut res: Map<String, Value>, ok: bool, commands: Vec<String>, diag: String| {
        res.insert("ok".into(), json!(ok));
        res.insert("commands".into(), json!(commands));
        res.insert("diagnostics".into(), json!(diag));
        Value::Object(res)
    };

    // 1) Assembly do compilador
    let mut s_cmd = base.clone();
    s_cmd.extend(sv(&["-S", lang.src, "-o", "prog.s"]));
    if intel {
        s_cmd.push("-masm=intel".into());
    }
    commands.push(shown(&s_cmd));
    let r = run(&s_cmd, w, COMPILE_TIMEOUT);
    diag.push_str(&r.stderr);
    if !r.ok() {
        return finish(res, false, commands, diag);
    }
    let s_text = fs::read_to_string(w.join("prog.s")).unwrap_or_default();
    let (mut asm, user_syms) = parse_s(&s_text, lang.src);
    let names: HashSet<String> = user_syms.into_iter().collect();

    // 2) Ligação estática (facilita gdb/qemu); cai para dinâmica se faltar libc.a.
    let mut link = base.clone();
    link.extend(sv(&[lang.src, lang.nobuf, "-o", "prog", "-fuse-ld=lld", "-static", "-lm"]));
    let mut r = run(&link, w, COMPILE_TIMEOUT);
    let mut is_static = true;
    if !r.ok() {
        let alt: Vec<String> = link.iter().filter(|a| *a != "-static").cloned().collect();
        let r2 = run(&alt, w, COMPILE_TIMEOUT);
        if r2.ok() {
            link = alt;
            r = r2;
            is_static = false;
        }
    }
    commands.push(shown(&link));
    if !r.ok() {
        diag.push_str("\n[ligação] ");
        diag.push_str(&r.stderr);
        if lang.cxx {
            let map = demangler(&[&s_text], w);
            for l in asm.iter_mut() {
                let t = demangle(l["t"].as_str().unwrap_or(""), &map);
                l["t"] = json!(t);
            }
        }
        res.insert("asm".into(), json!(asm));
        res.insert("linkError".into(), json!(true));
        return finish(res, true, commands, diag);
    }

    // 3) Funções do usuário, desmontagem e seções do binário
    let binary = w.join("prog").to_string_lossy().into_owned();
    let nm = run(&[cfg.nm.clone(), "--print-size".into(), "--defined-only".into(), binary.clone()], w, COMPILE_TIMEOUT);
    let mut funcs = parse_nm(&nm.stdout, &names);
    let mut dis_cmd = vec![cfg.objdump.clone(), "-d".into(), "-l".into(), "--no-show-raw-insn".into()];
    dis_cmd.push(format!(
        "--disassemble-symbols={}",
        funcs.iter().map(|f| f.sym.as_str()).collect::<Vec<_>>().join(",")
    ));
    if intel {
        dis_cmd.push("--x86-asm-syntax=intel".into());
    }
    dis_cmd.push(binary.clone());
    commands.push(shown(&dis_cmd));
    let dis_text = run(&dis_cmd, w, COMPILE_TIMEOUT).stdout;
    let mut disasm = parse_objdump(&dis_text, lang.src);
    let sections = parse_sections(&run(&[cfg.objdump.clone(), "-h".into(), binary], w, COMPILE_TIMEOUT).stdout);

    // instruções (endereço, texto original) para o trace detectar chamadas
    let insns: Vec<(u64, String)> = disasm
        .iter()
        .filter(|a| a["kind"] == "insn")
        .map(|a| (a["addr"].as_u64().unwrap_or(0), a["t"].as_str().unwrap_or("").to_string()))
        .collect();

    if lang.cxx {
        let map = demangler(&[&s_text, &dis_text], w);
        for l in asm.iter_mut().chain(disasm.iter_mut()) {
            let t = demangle(l["t"].as_str().unwrap_or(""), &map);
            l["t"] = json!(t);
        }
        for f in funcs.iter_mut() {
            f.name = demangle(&f.sym, &map);
        }
    }

    res.insert("asm".into(), json!(asm));
    res.insert("disasm".into(), json!(disasm));
    res.insert(
        "functions".into(),
        json!(funcs.iter().map(|f| json!({"name": f.name, "start": f.start, "end": f.end})).collect::<Vec<_>>()),
    );
    res.insert("sections".into(), json!(sections));
    res.insert("static".into(), json!(is_static));

    if let Some(ctl) = ctl.filter(|_| req["trace"].as_bool().unwrap_or(false)) {
        ctl.set_base(finish(res.clone(), true, commands.clone(), diag.clone()));
        let t = trace(w, arch, lang.src, "main", &funcs, &insns, max_steps, is_static, ctl);
        res.insert("trace".into(), t);
    }
    finish(res, true, commands, diag)
}

// ---------------------------------------------------------------------------
// Trace via GDB/MI
// ---------------------------------------------------------------------------
/// Tipos cujo valor é um endereço: `int *` (C/C++), `&T`, `*mut T` e `Box<T>` (Rust).
fn is_ptr_type(ty: &str) -> bool {
    let t = ty.trim();
    t.ends_with('*') || t.starts_with('&') || t.starts_with("*mut ") || t.starts_with("*const ") || t.starts_with("alloc::boxed::Box<")
}

struct GlobalInfo {
    name: String,
    ty: String,
    addr: u64,
    size: u64,
}

struct Tracer<'a> {
    g: Gdb,
    x86: bool,
    src: &'a str,
    entry: &'a str,
    funcs: &'a [Func],
    insns: &'a [(u64, String)],
    stdout_path: PathBuf,
    ctl: &'a mut InputCtl,
    pid: Option<u32>, // processo que lê o stdin (o programa ou o qemu)
    meta: Value,
    regs: Vec<(String, String)>, // (nome, número no gdb)
    // (função, topo do frame, fp, sp, nome) → (endereço, tamanho, tipo). O sp entra na
    // chave porque, em funções folha ARM64, as variáveis são relativas ao sp.
    var_cache: HashMap<(String, u64, u64, u64, String), (Option<u64>, u64, String)>,
    globals: Option<Vec<GlobalInfo>>,
    steps: Vec<Value>,
    status: String,
    exit_code: Option<i64>,
    error: Option<String>,
}

impl<'a> Tracer<'a> {
    fn sp_name(&self) -> &'static str {
        if self.x86 { "rsp" } else { "sp" }
    }
    fn fp_name(&self) -> &'static str {
        if self.x86 { "rbp" } else { "x29" }
    }

    fn in_user(&self, pc: u64) -> bool {
        self.funcs.iter().any(|f| pc >= f.start && pc < f.end)
    }

    fn insn_index(&self, pc: u64) -> Option<usize> {
        self.insns.binary_search_by_key(&pc, |(a, _)| *a).ok()
    }

    fn is_call(&self, pc: u64) -> bool {
        self.insn_index(pc)
            .and_then(|i| self.insns[i].1.split_whitespace().next())
            .map(|m| matches!(m, "call" | "callq" | "bl" | "blr"))
            .unwrap_or(false)
    }

    fn next_addr(&mut self, pc: u64) -> Option<u64> {
        if let Some(i) = self.insn_index(pc) {
            if let Some((a, _)) = self.insns.get(i + 1) {
                return Some(*a);
            }
        }
        if !self.x86 {
            return Some(pc + 4);
        }
        let v = self.g.cmd(&format!("-data-disassemble -s {pc:#x} -e {:#x} -- 0", pc + 16)).ok()?;
        v["asm_insns"].as_array()?.get(1).and_then(|i| parse_num(i["address"].as_str()?))
    }

    fn setup(&mut self) -> Result<(), String> {
        for c in [
            "set pagination off",
            "set confirm off",
            "set width 0",
            "set debuginfod enabled off",
            "set print elements 64",
            "set print repeats 16",
            // inclui o chamador de main (libc) no backtrace: o sp dele é o topo do frame de main
            "set backtrace past-main on",
        ] {
            let _ = self.g.console(c);
        }
        Ok(())
    }

    fn start_native(&mut self) -> Result<Value, String> {
        self.setup()?;
        self.g.cmd("-file-exec-and-symbols prog")?;
        let _ = self.g.console("set startup-with-shell on");
        let _ = self.g.console("set disable-randomization on");
        self.g.cmd("-exec-arguments < stdin.fifo >> stdout.txt 2>&1")?;
        self.g.cmd(&format!("-break-insert -t *{}", self.entry))?;
        self.g.cmd("-exec-run")?;
        let stop = self.g.wait_stopped()?;
        let groups = self.g.cmd("-list-thread-groups")?;
        self.pid = groups["groups"][0]["pid"].as_str().and_then(|p| p.parse().ok());
        Ok(stop)
    }

    /// Espera o programa parar; se ele bloquear lendo o stdin, pede a entrada
    /// ao usuário (o tempo de espera não conta para os limites).
    fn wait_stopped_input(&mut self) -> Result<Value, String> {
        loop {
            if let Some(v) = self.g.wait_stopped_for(Duration::from_millis(150))? {
                return Ok(v);
            }
            if self.pid.is_some_and(blocked_on_stdin) {
                let out = read_text(&self.stdout_path, 200_000);
                let d = self.ctl.request_input(&self.steps, out, &self.meta)?;
                self.g.deadline += d;
            }
        }
    }

    fn start_remote(&mut self, port: u16, sysroot: &str) -> Result<Value, String> {
        self.setup()?;
        self.g.console("set architecture aarch64")?;
        let _ = self.g.console(&format!("set sysroot {sysroot}"));
        self.g.cmd("-file-exec-and-symbols prog")?;
        let mut last = String::new();
        let mut connected = false;
        for _ in 0..50 {
            match self.g.cmd(&format!("-target-select remote 127.0.0.1:{port}")) {
                Ok(_) => {
                    connected = true;
                    break;
                }
                Err(e) => {
                    last = e;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        if !connected {
            return Err(format!("não foi possível conectar ao qemu-aarch64: {last}"));
        }
        self.g.cmd(&format!("-break-insert -t *{}", self.entry))?;
        self.g.cmd("-exec-continue")?;
        self.g.wait_stopped()
    }

    fn init_regs(&mut self) -> Result<(), String> {
        let wanted: Vec<String> = if self.x86 {
            sv(&[
                "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "r8", "r9", "r10", "r11", "r12", "r13",
                "r14", "r15", "rip", "eflags",
            ])
        } else {
            let mut v: Vec<String> = (0..31).map(|i| format!("x{i}")).collect();
            v.extend(sv(&["sp", "pc", "cpsr"]));
            v
        };
        let v = self.g.cmd("-data-list-register-names")?;
        let names: Vec<&str> = v["register-names"]
            .as_array()
            .map(|a| a.iter().map(|x| x.as_str().unwrap_or("")).collect())
            .unwrap_or_default();
        self.regs = wanted
            .into_iter()
            .filter_map(|w| names.iter().position(|n| *n == w).map(|i| (w, i.to_string())))
            .collect();
        Ok(())
    }

    /// Lê o estado completo no ponto atual.
    fn record(&mut self) -> Result<Value, String> {
        // registradores
        let idx: Vec<&str> = self.regs.iter().map(|(_, i)| i.as_str()).collect();
        let rv = self.g.cmd(&format!("-data-list-register-values --skip-unavailable x {}", idx.join(" ")))?;
        let by_num: HashMap<String, String> = rv["register-values"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|r| (r["number"].as_str().unwrap_or("").to_string(), r["value"].as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let mut regs = Map::new();
        for (name, num) in &self.regs {
            if let Some(v) = by_num.get(num) {
                let shown = parse_num(v).filter(|_| v.starts_with("0x")).map(|n| format!("{n:#018x}")).unwrap_or(v.clone());
                regs.insert(name.clone(), json!(shown));
            }
        }
        let reg = |n: &str| regs.get(n).and_then(|v| v.as_str()).and_then(parse_num);
        let sp0 = reg(self.sp_name()).unwrap_or(0);
        let fp0 = reg(self.fp_name());

        // frames
        let fv = self.g.cmd(&format!("-stack-list-frames 0 {}", MAX_FRAMES))?;
        let list: Vec<Value> = fv["stack"].as_array().cloned().unwrap_or_default();
        let addr_of = |f: &Value| f["addr"].as_str().and_then(parse_num).unwrap_or(0);
        let n_user = list.iter().take_while(|f| self.in_user(addr_of(f))).count();

        // sp de cada frame (o sp do frame L+1 é o "topo" do frame L)
        let mut sps: Vec<Option<u64>> = vec![Some(sp0)];
        for l in 1..=n_user.min(list.len().saturating_sub(1)) {
            sps.push(self.g.eval(l, "$sp").ok().and_then(|s| parse_num(&s)));
        }

        let mut frames = Vec::new();
        for l in 0..n_user {
            let f = &list[l];
            // código de biblioteca expandido inline numa função do usuário (ex.: vec!
            // e Box::new no Rust): pertence ao frame do usuário logo abaixo
            if f["file"].as_str().is_some_and(|file| !file.ends_with(self.src)) {
                continue;
            }
            let func = f["func"].as_str().unwrap_or("?").to_string();
            let line = f["line"].as_str().and_then(|s| s.parse::<u64>().ok());
            let sp = sps.get(l).copied().flatten();
            let fp = if l == 0 {
                fp0
            } else {
                self.g.eval(l, &format!("${}", self.fp_name())).ok().and_then(|s| parse_num(&s))
            };
            let top = sps.get(l + 1).copied().flatten();
            let vars = self.frame_vars(l, &func, top, fp, sp)?;
            frames.push(json!({
                "func": func, "pc": addr_of(f), "line": line,
                "sp": sp, "fp": fp, "top": top, "vars": vars,
            }));
        }

        // janela de bytes da pilha
        let red = if self.x86 { 128 } else { 0 };
        let lo = sp0.saturating_sub(red) & !7;
        let hi = sps.iter().skip(1).flatten().max().copied().unwrap_or(sp0 + 256);
        let hi = ((hi + 7) & !7).min(lo + MAX_STACK).max(lo);
        let stack_hex = self.g.read_hex(lo, hi - lo).unwrap_or_default();

        // globais
        if self.globals.is_none() {
            self.globals = Some(self.load_globals());
        }
        let mut globals = Vec::new();
        let ginfo: Vec<(String, String, u64, u64)> = self
            .globals
            .as_ref()
            .unwrap()
            .iter()
            .map(|g| (g.name.clone(), g.ty.clone(), g.addr, g.size))
            .collect();
        for (name, ty, addr, size) in ginfo {
            let value = self.g.eval(0, &name).unwrap_or_else(|e| format!("<{e}>"));
            let hex = self.g.read_hex(addr, size.min(256));
            let mut item = json!({"name": name, "type": ty, "addr": addr, "size": size, "value": cap(&value, 240), "hex": hex});
            if is_ptr_type(&ty) {
                item["ptr"] = json!(parse_num(&value));
            }
            globals.push(item);
        }

        // memória apontada por ponteiros fora da pilha (heap, .rodata...)
        let mut pointees: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();
        let candidates: Vec<(u64, String, String)> = frames
            .iter()
            .flat_map(|f| f["vars"].as_array().cloned().unwrap_or_default())
            .chain(globals.iter().cloned())
            .filter_map(|v| {
                let p = v["ptr"].as_u64()?;
                Some((p, v["name"].as_str()?.to_string(), v["type"].as_str().unwrap_or("").to_string()))
            })
            .collect();
        for (p, from, ty) in candidates {
            if p == 0 || (p >= lo && p < hi) || !seen.insert(p) {
                continue;
            }
            if let Some(h) = self.g.read_hex(p, POINTEE_BYTES).or_else(|| self.g.read_hex(p, 16)) {
                pointees.push(json!({"addr": p, "hex": h, "from": from, "type": ty}));
            }
        }

        // linha do primeiro frame no fonte do usuário (pula os frames inline de biblioteca)
        let line0 = list.iter().take(n_user.max(1)).find_map(|f| {
            let file = f["file"].as_str().unwrap_or("");
            file.ends_with(self.src).then(|| f["line"].as_str()?.parse::<u64>().ok()).flatten()
        });
        Ok(json!({
            "pc": list.first().map(addr_of).unwrap_or(0),
            "line": line0,
            "func": list.first().and_then(|f| f["func"].as_str()).unwrap_or("?"),
            "regs": regs,
            "frames": frames,
            "stack": {"lo": lo, "hex": stack_hex},
            "globals": globals,
            "pointees": pointees,
            "outLen": file_len(&self.stdout_path),
        }))
    }

    fn frame_vars(
        &mut self,
        level: usize,
        func: &str,
        top: Option<u64>,
        fp: Option<u64>,
        sp: Option<u64>,
    ) -> Result<Vec<Value>, String> {
        let v = self.g.cmd(&format!("-stack-list-variables --thread 1 --frame {level} --all-values"))?;
        let vars = v["variables"].as_array().cloned().unwrap_or_default();
        let mut types: Option<HashMap<String, String>> = None;
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for var in vars {
            let name = var["name"].as_str().unwrap_or("").to_string();
            if name.is_empty() || !seen.insert(name.clone()) {
                continue;
            }
            let value = var["value"].as_str().unwrap_or("").to_string();
            let key = (func.to_string(), top.unwrap_or(0), fp.unwrap_or(0), sp.unwrap_or(0), name.clone());
            if !self.var_cache.contains_key(&key) {
                if types.is_none() {
                    let sv = self.g.cmd(&format!("-stack-list-variables --thread 1 --frame {level} --simple-values"))?;
                    types = Some(
                        sv["variables"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .map(|x| {
                                        (x["name"].as_str().unwrap_or("").to_string(), x["type"].as_str().unwrap_or("").to_string())
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    );
                }
                let addr = self.g.eval(level, &format!("&({name})")).ok().and_then(|s| parse_num(&s));
                let size = self.g.eval(level, &format!("sizeof({name})")).ok().and_then(|s| parse_num(&s)).unwrap_or(0);
                let ty = types.as_ref().and_then(|t| t.get(&name).cloned()).unwrap_or_default();
                self.var_cache.insert(key.clone(), (addr, size, ty));
            }
            let (addr, size, ty) = self.var_cache[&key].clone();
            let mut item = json!({
                "name": name, "type": ty, "size": size, "addr": addr,
                "value": cap(&value, 240), "arg": var["arg"] == "1",
            });
            if is_ptr_type(&ty) {
                item["ptr"] = json!(parse_num(&value));
            }
            out.push(item);
        }
        Ok(out)
    }

    fn load_globals(&mut self) -> Vec<GlobalInfo> {
        let Ok(v) = self.g.cmd("-symbol-info-variables") else { return vec![] };
        let mut out = Vec::new();
        let files = v["symbols"]["debug"].as_array().cloned().unwrap_or_default();
        for f in files {
            if !f["filename"].as_str().unwrap_or("").ends_with(self.src) {
                continue;
            }
            for s in f["symbols"].as_array().cloned().unwrap_or_default() {
                let name = s["name"].as_str().unwrap_or("").to_string();
                let Some(addr) = self.g.eval(0, &format!("&({name})")).ok().and_then(|x| parse_num(&x)) else { continue };
                let size = self.g.eval(0, &format!("sizeof({name})")).ok().and_then(|x| parse_num(&x)).unwrap_or(0);
                out.push(GlobalInfo { name, ty: s["type"].as_str().unwrap_or("").to_string(), addr, size });
            }
        }
        out
    }

    /// Laço principal: stepi a partir de main.
    fn run_loop(&mut self, mut stop: Value, max_steps: usize, time_limit: Duration) {
        let start = Instant::now();
        let mut prev_pc: Option<u64> = None;
        loop {
            let reason = stop["reason"].as_str().unwrap_or("").to_string();
            if reason.starts_with("exited") {
                self.status = "exited".into();
                self.exit_code = Some(stop["exit-code"].as_str().and_then(|c| i64::from_str_radix(c, 8).ok()).unwrap_or(0));
                return;
            }
            let pc = stop["frame"]["addr"].as_str().and_then(parse_num).unwrap_or(0);
            if self.steps.len() >= max_steps {
                self.status = "limit".into();
                return;
            }
            if start.elapsed().saturating_sub(self.ctl.paused()) > time_limit {
                self.status = "timeout".into();
                return;
            }
            if !self.in_user(pc) {
                if let Some(p) = prev_pc.filter(|p| self.is_call(*p)) {
                    // Entrou numa função de biblioteca: executa-a até o retorno.
                    let Some(ret) = self.next_addr(p) else {
                        self.status = "error".into();
                        self.error = Some("não foi possível achar o endereço de retorno".into());
                        return;
                    };
                    let r = self
                        .g
                        .cmd(&format!("-break-insert -t *{ret:#x}"))
                        .and_then(|_| self.g.cmd("-exec-continue"))
                        .and_then(|_| self.wait_stopped_input());
                    match r {
                        Ok(s) => stop = s,
                        Err(e) => {
                            self.status = "error".into();
                            self.error = Some(e);
                            return;
                        }
                    }
                    prev_pc = None;
                    continue;
                }
                self.status = "returned".into(); // main retornou para a libc
                return;
            }
            match self.record() {
                Ok(s) => self.steps.push(s),
                Err(e) => {
                    self.status = "error".into();
                    self.error = Some(format!("erro ao ler o estado no passo {}: {e}", self.steps.len()));
                    return;
                }
            }
            if reason == "signal-received" {
                self.status = "signal".into();
                self.error = Some(format!(
                    "o programa recebeu o sinal {} ({})",
                    stop["signal-name"].as_str().unwrap_or("?"),
                    stop["signal-meaning"].as_str().unwrap_or("")
                ));
                return;
            }
            prev_pc = Some(pc);
            match self.g.cmd("-exec-step-instruction").and_then(|_| self.g.wait_stopped()) {
                Ok(s) => stop = s,
                Err(e) => {
                    self.status = "error".into();
                    self.error = Some(e);
                    return;
                }
            }
        }
    }

    fn finish(&mut self) {
        match self.status.as_str() {
            "returned" => {
                if let Ok(s) = self.g.cmd("-exec-continue").and_then(|_| self.wait_stopped_input()) {
                    if s["reason"].as_str().unwrap_or("").starts_with("exited") {
                        self.exit_code =
                            Some(s["exit-code"].as_str().and_then(|c| i64::from_str_radix(c, 8).ok()).unwrap_or(0));
                    }
                }
            }
            "exited" => {}
            _ => {
                // se o programa ainda está rodando (ex.: bloqueado no read após
                // cancelamento ou tempo esgotado), o gdb não atende comandos até
                // ele parar: mata o processo diretamente
                if let Some(pid) = self.pid {
                    let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
                    let _ = self.g.wait_stopped_for(Duration::from_secs(2));
                }
                let _ = self.g.console("kill");
            }
        }
    }
}

fn spawn_qemu(w: &Path, port: u16, is_static: bool, fifo: &Path) -> Result<Child, String> {
    let cfg = config();
    let qemu = cfg.qemu.clone().ok_or_else(|| {
        "qemu-aarch64 não encontrado. Instale o pacote qemu-user (ou defina QEMU_AARCH64) para executar binários ARM64."
            .to_string()
    })?;
    let fin = File::open(fifo).map_err(|e| e.to_string())?;
    // em modo append, o eco da entrada (session.rs) fica na ordem certa
    let fout = OpenOptions::new().append(true).open(w.join("stdout.txt")).map_err(|e| e.to_string())?;
    let ferr = fout.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&qemu);
    cmd.arg("-g").arg(port.to_string());
    if !is_static {
        cmd.arg("-L").arg(&cfg.sysroot);
    }
    cmd.arg(w.join("prog"))
        .current_dir(w)
        .stdin(Stdio::from(fin))
        .stdout(Stdio::from(fout))
        .stderr(Stdio::from(ferr))
        .spawn()
        .map_err(|e| format!("falha ao executar {qemu}: {e}"))
}

/// Executa `prog` (em `w`) instrução a instrução a partir do símbolo `entry`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn trace(
    w: &Path,
    arch: &str,
    src: &str,
    entry: &str,
    funcs: &[Func],
    insns: &[(u64, String)],
    max_steps: usize,
    is_static: bool,
    ctl: &mut InputCtl,
) -> Value {
    let cfg = config();
    let x86 = arch == "x86_64";
    let meta = json!({
        "kind": "native",
        "regs": if x86 {
            json!(["rax","rbx","rcx","rdx","rsi","rdi","rbp","rsp","r8","r9","r10","r11","r12","r13","r14","r15","rip","eflags"])
        } else {
            let mut v: Vec<String> = (0..31).map(|i| format!("x{i}")).collect();
            v.extend(sv(&["sp", "pc", "cpsr"]));
            json!(v)
        },
        "sp": if x86 { "rsp" } else { "sp" },
        "fp": if x86 { "rbp" } else { "x29" },
        "pc": if x86 { "rip" } else { "pc" },
        "redZone": if x86 { 128 } else { 0 },
    });
    let deadline = Instant::now() + Duration::from_secs(TRACE_SECS);
    let stdout_path = w.join("stdout.txt");

    let fifo = match ctl.open_stdin(w) {
        Ok(f) => f,
        Err(e) => return json!({"steps": [], "status": "error", "error": e, "meta": meta, "stdout": ""}),
    };
    let mut qemu = None;
    let port = free_port();
    if !x86 {
        match spawn_qemu(w, port, is_static, &fifo) {
            Ok(c) => qemu = Some(c),
            Err(e) => return json!({"steps": [], "status": "error", "error": e, "meta": meta, "stdout": ""}),
        }
    }

    let g = match Gdb::spawn(&cfg.gdb, w, deadline) {
        Ok(g) => g,
        Err(e) => {
            if let Some(mut q) = qemu {
                let _ = q.kill();
            }
            return json!({"steps": [], "status": "error", "error": e, "meta": meta, "stdout": ""});
        }
    };
    let mut t = Tracer {
        g,
        x86,
        src,
        entry,
        funcs,
        insns,
        stdout_path: stdout_path.clone(),
        pid: qemu.as_ref().map(|q| q.id()),
        meta: meta.clone(),
        ctl,
        regs: vec![],
        var_cache: HashMap::new(),
        globals: None,
        steps: vec![],
        status: String::new(),
        exit_code: None,
        error: None,
    };
    let sysroot = if is_static { "/".to_string() } else { cfg.sysroot.clone() };
    let started = if x86 { t.start_native() } else { t.start_remote(port, &sysroot) };
    match started.and_then(|s| t.init_regs().map(|_| s)) {
        Ok(stop) => {
            t.run_loop(stop, max_steps, Duration::from_secs(TRACE_SECS - 20));
            t.finish();
        }
        Err(e) => {
            t.status = "error".into();
            t.error = Some(format!("falha ao iniciar o programa: {e}"));
        }
    }
    let log = cap(&t.g.log, 4000);
    let result = json!({
        "steps": t.steps, "status": t.status, "error": t.error, "exitCode": t.exit_code,
        "meta": meta, "log": log,
    });
    drop(t);
    if let Some(mut q) = qemu {
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end && matches!(q.try_wait(), Ok(None)) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = q.kill();
        let _ = q.wait();
    }
    let mut result = result;
    result["stdout"] = json!(read_text(&stdout_path, 200_000));
    result
}
