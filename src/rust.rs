//! Rust: compila com rustc (x86_64 nativo ou AArch64 cross), mostra o assembly
//! das funções do usuário e executa instrução a instrução via GDB/MI, com o
//! mesmo parser e trace do C/C++ (native.rs).

use crate::config;
use crate::native::{demangle, demangler, parse_nm, parse_objdump, parse_s, parse_sections, trace};
use crate::session::InputCtl;
use crate::util::*;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

const COMPILE_TIMEOUT: Duration = Duration::from_secs(90);
const SRC: &str = "prog.rs";
const CRATE: &str = "prog";

fn opt_level(o: &str) -> Option<&'static str> {
    Some(match o {
        "-O0" => "0",
        "-O1" => "1",
        "-O2" => "2",
        "-O3" => "3",
        "-Os" => "s",
        _ => return None,
    })
}

/// O `rustc --emit asm` inclui todas as instâncias genéricas da std usadas
/// pelo programa (milhares de linhas); mantém só as funções do usuário.
fn keep_user_fns(asm: Vec<Value>, user: &HashSet<String>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut inside = false;
    for l in asm {
        let label = (l["kind"] == "label").then(|| {
            l["t"]
                .as_str()
                .unwrap_or("")
                .trim()
                .trim_end_matches(':')
                .to_string()
        });
        if let Some(name) = &label {
            if user.contains(name) {
                if !out.is_empty() {
                    out.push(json!({"t": "", "kind": "comment", "cline": null}));
                }
                inside = true;
            }
        }
        let end = label
            .as_deref()
            .map(|n| n.starts_with(".Lfunc_end"))
            .unwrap_or(false);
        if inside {
            out.push(l);
        }
        if end {
            inside = false;
        }
    }
    out
}

/// O rustup instala a std de cada alvo separadamente.
fn has_target(rustc: &str, target: &str, cwd: &Path) -> bool {
    let r = run(
        &[rustc.to_string(), "--print".into(), "sysroot".into()],
        cwd,
        Duration::from_secs(20),
    );
    Path::new(r.stdout.trim())
        .join("lib/rustlib")
        .join(target)
        .is_dir()
}

/// `ctl` existe quando o trace foi pedido (entrada interativa, ver session.rs).
pub fn build(req: &Value, ctl: Option<&mut InputCtl>) -> Value {
    let cfg = config();
    let arch = req["arch"].as_str().unwrap_or("x86_64");
    let intel = arch == "x86_64" && req["syntax"] == "intel";
    let opt = opt_level(req["opt"].as_str().unwrap_or("-O0")).unwrap_or("0");
    let max_steps = req["maxSteps"].as_u64().unwrap_or(3000).clamp(1, 20000) as usize;

    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => {
            return json!({"ok": false, "diagnostics": format!("erro ao criar diretório temporário: {e}")})
        }
    };
    let w = dir.path();
    let code = req["code"].as_str().unwrap_or("");
    if let Err(e) = fs::write(w.join(SRC), code) {
        return json!({"ok": false, "diagnostics": format!("erro ao gravar arquivos: {e}")});
    }

    let mut target = vec![];
    match arch {
        "x86_64" => {}
        "arm64" => {
            const T: &str = "aarch64-unknown-linux-gnu";
            if !has_target(&cfg.rustc, T, w) {
                return json!({"ok": false, "lang": "rust", "diagnostics": format!(
                    "a biblioteca padrão do Rust para {T} não está instalada.\nInstale com: rustup target add {T}"
                )});
            }
            target = sv(&["--target", T, "-C", "linker=aarch64-linux-gnu-gcc"]);
        }
        _ => return json!({"ok": false, "diagnostics": "arquitetura inválida"}),
    }
    let rustc_cmd = |is_static: bool| {
        let mut cmd = vec![cfg.rustc.clone()];
        cmd.extend(target.iter().cloned());
        cmd.extend(sv(&["--edition", "2024", "--crate-name", CRATE, "-g"]));
        // avisos do ld sobre getaddrinfo/getpwuid_r da std em binário estático
        // (glibc NSS): não afetam os programas do visualizador
        cmd.extend(sv(&["-A", "linker_messages"]));
        let mut flags = vec![
            format!("opt-level={opt}"),
            "force-frame-pointers=yes".into(),
            // um único .s com todas as funções
            "codegen-units=1".into(),
            // nomes v0 (_R...) são demangled sem o hash: prog::main, prog::main::{closure#0}
            "symbol-mangling-version=v0".into(),
            // sem PIE: os endereços do nm/objdump são os mesmos da execução
            "relocation-model=static".into(),
        ];
        if is_static {
            // binário estático, como no C/C++ (facilita gdb/qemu)
            flags.push("target-feature=+crt-static".into());
        }
        if intel {
            flags.push("llvm-args=-x86-asm-syntax=intel".into());
        }
        for f in flags {
            cmd.push("-C".into());
            cmd.push(f);
        }
        cmd.extend(sv(&["--emit=asm=prog.s,link=prog", SRC]));
        cmd
    };

    let mut res = Map::new();
    res.insert("lang".into(), json!("rust"));
    res.insert("arch".into(), json!(arch));
    let finish = |mut res: Map<String, Value>, ok: bool, commands: Vec<String>, diag: String| {
        res.insert("ok".into(), json!(ok));
        res.insert("commands".into(), json!(commands));
        res.insert("diagnostics".into(), json!(diag));
        Value::Object(res)
    };

    // 1) Compilação: assembly + binário. Sem a libc estática, liga dinamicamente.
    let mut cmd = rustc_cmd(true);
    let mut r = run(&cmd, w, COMPILE_TIMEOUT);
    let mut is_static = true;
    if !r.ok() && w.join("prog.s").exists() {
        let alt = rustc_cmd(false);
        let r2 = run(&alt, w, COMPILE_TIMEOUT);
        if r2.ok() {
            cmd = alt;
            r = r2;
            is_static = false;
        }
    }
    let mut commands = vec![shown(&cmd)];
    let diag = r.stderr.clone();
    let s_text = fs::read_to_string(w.join("prog.s")).unwrap_or_default();
    if s_text.is_empty() {
        return finish(res, false, commands, diag);
    }
    let (asm, user_syms) = parse_s(&s_text, SRC);
    // o `main` em C gerado pelo rustc não tem informação de linha: fica de fora
    let names: HashSet<String> = user_syms.into_iter().filter(|n| n != "main").collect();
    let mut asm = keep_user_fns(asm, &names);

    if !r.ok() {
        let map = demangler(&[&s_text], w);
        for l in asm.iter_mut() {
            l["t"] = json!(demangle(l["t"].as_str().unwrap_or(""), &map));
        }
        res.insert("asm".into(), json!(asm));
        res.insert("linkError".into(), json!(true));
        return finish(res, true, commands, diag);
    }

    // 2) Funções do usuário, desmontagem e seções do binário
    let binary = w.join("prog").to_string_lossy().into_owned();
    let nm = run(
        &[
            cfg.nm.clone(),
            "--print-size".into(),
            "--defined-only".into(),
            binary.clone(),
        ],
        w,
        COMPILE_TIMEOUT,
    );
    let mut funcs = parse_nm(&nm.stdout, &names);
    let mut dis_cmd = vec![
        cfg.objdump.clone(),
        "-d".into(),
        "-l".into(),
        "--no-show-raw-insn".into(),
    ];
    dis_cmd.push(format!(
        "--disassemble-symbols={}",
        funcs
            .iter()
            .map(|f| f.sym.as_str())
            .collect::<Vec<_>>()
            .join(",")
    ));
    if intel {
        dis_cmd.push("--x86-asm-syntax=intel".into());
    }
    dis_cmd.push(binary.clone());
    commands.push(shown(&dis_cmd));
    let dis_text = run(&dis_cmd, w, COMPILE_TIMEOUT).stdout;
    let mut disasm = parse_objdump(&dis_text, SRC);
    let sections = parse_sections(
        &run(
            &[cfg.objdump.clone(), "-h".into(), binary],
            w,
            COMPILE_TIMEOUT,
        )
        .stdout,
    );

    let insns: Vec<(u64, String)> = disasm
        .iter()
        .filter(|a| a["kind"] == "insn")
        .map(|a| {
            (
                a["addr"].as_u64().unwrap_or(0),
                a["t"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();

    let map = demangler(&[&s_text, &dis_text], w);
    for l in asm.iter_mut().chain(disasm.iter_mut()) {
        l["t"] = json!(demangle(l["t"].as_str().unwrap_or(""), &map));
    }
    for f in funcs.iter_mut() {
        f.name = demangle(&f.sym, &map);
    }
    // o gdb em modo Rust não resolve o nome mangled: entra pelo endereço
    let entry = funcs
        .iter()
        .find(|f| f.name == format!("{CRATE}::main"))
        .map(|f| format!("{:#x}", f.start));

    res.insert("asm".into(), json!(asm));
    res.insert("disasm".into(), json!(disasm));
    res.insert(
        "functions".into(),
        json!(funcs
            .iter()
            .map(|f| json!({"name": f.name, "start": f.start, "end": f.end}))
            .collect::<Vec<_>>()),
    );
    res.insert("sections".into(), json!(sections));
    res.insert("static".into(), json!(is_static));

    if let Some(ctl) = ctl.filter(|_| req["trace"].as_bool().unwrap_or(false)) {
        ctl.set_base(finish(res.clone(), true, commands.clone(), diag.clone()));
        let t = match &entry {
            Some(sym) => trace(w, arch, SRC, sym, &funcs, &insns, max_steps, is_static, ctl),
            None => {
                json!({"steps": [], "status": "error", "error": "função main não encontrada", "stdout": ""})
            }
        };
        res.insert("trace".into(), t);
    }
    finish(res, true, commands, diag)
}
