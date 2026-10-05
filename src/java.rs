//! Java: compila com javac -g, mostra o bytecode (javap -c) e executa
//! instrução de bytecode a instrução com o jdb (stepi), anexado a uma JVM
//! iniciada com o agente JDWP.

use crate::config;
use crate::session::InputCtl;
use crate::util::*;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

const COMPILE_TIMEOUT: Duration = Duration::from_secs(90);
const TRACE_SECS: u64 = 120;
const MAX_JAVA_FRAMES: usize = 24;
const MAX_DUMP_ARRAY: usize = 64;

macro_rules! rx {
    ($pat:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new($pat).unwrap())
    }};
}

struct LocalVar {
    start: u32,
    len: u32,
    slot: u32,
    name: String,
    sig: String,
}

struct StaticField {
    class: String,
    name: String,
    ty: String,
}

/// Descritor JVM → tipo Java legível ("I" → int, "[Ljava/lang/String;" → String[]).
fn descriptor_to_type(d: &str) -> String {
    let dims = d.chars().take_while(|c| *c == '[').count();
    let base = &d[dims..];
    let t = match base.chars().next() {
        Some('I') => "int".to_string(),
        Some('J') => "long".to_string(),
        Some('Z') => "boolean".to_string(),
        Some('B') => "byte".to_string(),
        Some('C') => "char".to_string(),
        Some('S') => "short".to_string(),
        Some('F') => "float".to_string(),
        Some('D') => "double".to_string(),
        Some('L') => {
            let full = base
                .trim_start_matches('L')
                .trim_end_matches(';')
                .replace('/', ".");
            full.rsplit('.').next().unwrap_or(&full).to_string()
        }
        _ => base.to_string(),
    };
    format!("{t}{}", "[]".repeat(dims))
}

/// Intervalo (abre, fecha) das chaves do corpo que começa após `from`,
/// ignorando strings, caracteres e comentários.
fn body_range(code: &str, from: usize) -> Option<(usize, usize)> {
    let b = code.as_bytes();
    let mut i = from;
    let mut depth = 0usize;
    let mut open = None;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'{' => {
                if open.is_none() {
                    open = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && open.is_some() {
                    return Some((open.unwrap(), i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Analisa o código: pacote, classe com main e nome do arquivo .java.
fn analyze(code: &str) -> (String, String) {
    let pkg = rx!(r"(?m)^\s*package\s+([\w.]+)\s*;")
        .captures(code)
        .map(|c| c[1].to_string());
    let classes: Vec<(usize, String)> = rx!(r"\b(?:class|interface|enum|record)\s+([A-Za-z_]\w*)")
        .captures_iter(code)
        .map(|c| (c.get(0).unwrap().start(), c[1].to_string()))
        .collect();
    let main_pos = rx!(r"static\s+(?:public\s+)?void\s+main\s*\(")
        .find(code)
        .map(|m| m.start());
    // classes cujo corpo contém o main, da mais externa para a mais interna
    let chain: Vec<String> = match main_pos {
        Some(p) => classes
            .iter()
            .filter(|(s, _)| {
                body_range(code, *s)
                    .map(|(a, b)| a < p && p < b)
                    .unwrap_or(false)
            })
            .map(|(_, n)| n.clone())
            .collect(),
        None => vec![],
    };
    let main_class = if chain.is_empty() {
        classes
            .first()
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| "Main".to_string())
    } else {
        chain.join("$")
    };
    let file = rx!(r"public\s+(?:(?:final|abstract|sealed|strictfp)\s+)*(?:class|interface|enum|record)\s+([A-Za-z_]\w*)")
        .captures(code)
        .map(|c| c[1].to_string())
        .unwrap_or_else(|| main_class.clone());
    let fqn = match pkg {
        Some(p) => format!("{p}.{main_class}"),
        None => main_class,
    };
    (fqn, format!("{file}.java"))
}

fn class_files(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            class_files(&p, base, out);
        } else if p.extension().map(|x| x == "class").unwrap_or(false) {
            if let Ok(rel) = p.strip_prefix(base) {
                let s = rel
                    .to_string_lossy()
                    .trim_end_matches(".class")
                    .replace('/', ".");
                out.push(s);
            }
        }
    }
}

/// Saída do javap -c -l -p → linhas de bytecode, campos static e tabelas de variáveis locais.
fn parse_javap(text: &str) -> (Vec<Value>, Vec<StaticField>, HashMap<String, Vec<LocalVar>>) {
    #[derive(PartialEq)]
    enum Sec {
        None,
        Code,
        Lnt,
        Lvt,
        Other,
    }
    let mut out: Vec<Value> = Vec::new();
    let mut statics = Vec::new();
    let mut lvts: HashMap<String, Vec<LocalVar>> = HashMap::new();
    let mut cls = String::new();
    let mut method: Option<String> = None;
    let mut method_from = 0usize;
    let mut lnt: Vec<(u64, u64)> = Vec::new(); // (linha, bci inicial)
    let mut sec = Sec::None;
    let mut in_switch = false;

    fn close(out: &mut [Value], from: usize, lnt: &mut Vec<(u64, u64)>) {
        for v in out[from..].iter_mut() {
            if v["kind"] != "insn" {
                continue;
            }
            let bci = v["addr"].as_u64().unwrap_or(0);
            let line = lnt
                .iter()
                .filter(|(_, s)| *s <= bci)
                .max_by_key(|(_, s)| *s)
                .map(|(l, _)| *l);
            v["cline"] = json!(line);
        }
        lnt.clear();
    }

    for line in text.lines() {
        if line.starts_with("Compiled from") || line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let t = line.trim();
        if indent == 0 {
            if t == "}" {
                close(&mut out, method_from, &mut lnt);
                method = None;
                continue;
            }
            if let Some(c) = rx!(r"\b(?:class|interface|enum|record)\s+([\w$.]+)").captures(t) {
                close(&mut out, method_from, &mut lnt);
                method = None;
                cls = c[1].to_string();
                out.push(json!({"kind": "comment", "t": format!("// {}", t.trim_end_matches('{').trim()), "cline": null}));
            }
            continue;
        }
        if indent == 2 {
            // declaração de membro: método ou campo
            close(&mut out, method_from, &mut lnt);
            method = None;
            sec = Sec::None;
            in_switch = false;
            if t.starts_with("static {}") {
                let key = format!("{cls}.<clinit>");
                out.push(json!({"kind": "label", "t": format!("{cls}.<clinit>  // static {{}}"), "m": key, "cline": null}));
                method = Some(key);
                method_from = out.len();
            } else if let Some(p) = t.find('(') {
                let name = t[..p].split_whitespace().last().unwrap_or("?");
                let simple = cls.rsplit(['.', '$']).next().unwrap_or(&cls);
                let mname = if name == cls || name == simple || cls.ends_with(&format!("${name}")) {
                    "<init>".to_string()
                } else {
                    name.to_string()
                };
                let key = format!("{cls}.{mname}");
                out.push(json!({"kind": "label", "t": format!("{key}  // {}", t.trim_end_matches(';')), "m": key, "cline": null}));
                method = Some(key);
                method_from = out.len();
            } else if t.ends_with(';') && (t.contains(" static ") || t.starts_with("static ")) {
                let toks: Vec<&str> = t.trim_end_matches(';').split_whitespace().collect();
                if toks.len() >= 2 {
                    let name = toks[toks.len() - 1].to_string();
                    let ty = toks[toks.len() - 2];
                    let ty = ty.rsplit('.').next().unwrap_or(ty).to_string();
                    statics.push(StaticField {
                        class: cls.clone(),
                        name,
                        ty,
                    });
                }
            }
            continue;
        }
        let Some(key) = method.clone() else { continue };
        if indent == 4 {
            sec = match t {
                "Code:" => Sec::Code,
                "LineNumberTable:" => Sec::Lnt,
                "LocalVariableTable:" => Sec::Lvt,
                _ => Sec::Other,
            };
            continue;
        }
        match sec {
            Sec::Code => {
                if in_switch {
                    if t == "}" {
                        in_switch = false;
                    }
                    out.push(json!({"kind": "dir", "t": format!("      {t}"), "cline": null}));
                    continue;
                }
                if let Some(c) = rx!(r"^(\d+): (.*)$").captures(t) {
                    let text = rx!(r"\s+").replace_all(c[2].trim(), " ").into_owned();
                    if text.ends_with('{') {
                        in_switch = true;
                    }
                    let bci: u64 = c[1].parse().unwrap_or(0);
                    out.push(
                        json!({"kind": "insn", "t": text, "addr": bci, "m": key, "cline": null}),
                    );
                }
            }
            Sec::Lnt => {
                if let Some(c) = rx!(r"^line (\d+): (\d+)$").captures(t) {
                    lnt.push((c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0)));
                }
            }
            Sec::Lvt => {
                if let Some(c) = rx!(r"^(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s+(\S+)$").captures(t) {
                    lvts.entry(key).or_default().push(LocalVar {
                        start: c[1].parse().unwrap_or(0),
                        len: c[2].parse().unwrap_or(0),
                        slot: c[3].parse().unwrap_or(0),
                        name: c[4].to_string(),
                        sig: c[5].to_string(),
                    });
                }
            }
            _ => {}
        }
    }
    close(&mut out, method_from, &mut lnt);
    (out, statics, lvts)
}

/// `ctl` existe quando o trace foi pedido (entrada interativa, ver session.rs).
pub fn build(req: &Value, ctl: Option<&mut InputCtl>) -> Value {
    let cfg = config();
    let code = req["code"].as_str().unwrap_or("");
    let max_steps = req["maxSteps"].as_u64().unwrap_or(3000).clamp(1, 20000) as usize;
    let (main_class, file) = analyze(code);

    let dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => {
            return json!({"ok": false, "diagnostics": format!("erro ao criar diretório temporário: {e}")})
        }
    };
    let w = dir.path();
    let _ = fs::create_dir_all(w.join("classes"));
    if let Err(e) = fs::write(w.join(&file), code) {
        return json!({"ok": false, "diagnostics": format!("erro ao gravar arquivos: {e}")});
    }

    let mut commands = Vec::new();
    let javac = vec![
        cfg.javac.clone(),
        "-g".into(),
        "-encoding".into(),
        "UTF-8".into(),
        "-d".into(),
        "classes".into(),
        file.clone(),
    ];
    commands.push(shown(&javac));
    let r = run(&javac, w, COMPILE_TIMEOUT);
    let mut diag = r.stderr.clone() + &r.stdout;
    if !r.ok() {
        return json!({"ok": false, "lang": "java", "commands": commands, "diagnostics": diag});
    }

    let mut classes = Vec::new();
    class_files(&w.join("classes"), &w.join("classes"), &mut classes);
    classes.sort();
    let mut javap = vec![
        cfg.javap.clone(),
        "-c".into(),
        "-l".into(),
        "-p".into(),
        "-constants".into(),
        "-cp".into(),
        "classes".into(),
    ];
    javap.extend(classes.iter().cloned());
    commands.push(shown(&javap));
    let jp = run(&javap, w, COMPILE_TIMEOUT);
    if !jp.ok() {
        diag.push_str(&jp.stderr);
    }
    let (bytecode, statics, lvts) = parse_javap(&jp.stdout);

    let mut res = json!({
        "ok": true, "lang": "java", "arch": "jvm", "mainClass": main_class,
        "commands": commands, "diagnostics": diag, "disasm": bytecode,
        "classes": classes,
    });
    if let Some(ctl) = ctl.filter(|_| req["trace"].as_bool().unwrap_or(false)) {
        ctl.set_base(res.clone());
        let user: HashSet<String> = classes.into_iter().collect();
        res["trace"] = trace(w, &main_class, &user, &statics, &lvts, max_steps, ctl);
        if let Some(cmds) = res["trace"]["commands"].as_array().cloned() {
            if let Some(list) = res["commands"].as_array_mut() {
                list.extend(cmds);
            }
        }
    }
    res
}

// ---------------------------------------------------------------------------
// Sessão do jdb
// ---------------------------------------------------------------------------
struct Jdb {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    eof: bool,
    deadline: Instant,
}

enum Event {
    Stop {
        kind: String,
        class: String,
        method: String,
        line: u64,
        bci: u64,
    },
    Exit,
}

fn ends_with_prompt(s: &str) -> bool {
    rx!(r"(?:^|\n)[^\s\[\]>]+\[\d+\] $").is_match(s)
}

fn num(s: &str) -> u64 {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

impl Jdb {
    fn spawn(jdb: &str, port: u16, cwd: &Path, deadline: Instant) -> Result<Self, String> {
        let mut child = Command::new(jdb)
            .args(["-attach", &format!("127.0.0.1:{port}")])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("não foi possível executar {jdb}: {e}"))?;
        let stdin = child.stdin.take().unwrap();
        let (tx, rx) = channel();
        for mut src in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
            Box::new(child.stderr.take().unwrap()) as Box<dyn Read + Send>,
        ] {
            let tx = tx.clone();
            thread::spawn(move || {
                let mut b = [0u8; 8192];
                while let Ok(n) = src.read(&mut b) {
                    if n == 0 || tx.send(b[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
        }
        Ok(Jdb {
            child,
            stdin,
            rx,
            buf: Vec::new(),
            eof: false,
            deadline,
        })
    }

    fn send(&mut self, c: &str) -> Result<(), String> {
        if std::env::var_os("ASMVIZ_DEBUG").is_some() {
            eprintln!("jdb << {c}");
        }
        writeln!(self.stdin, "{c}").map_err(|e| format!("falha ao escrever no jdb: {e}"))
    }

    /// Acumula a saída até `done(texto)` ser verdadeiro (ou EOF / tempo esgotado).
    fn read_until(&mut self, done: impl Fn(&str) -> bool) -> Result<String, String> {
        self.read_until_or(done, None)
    }

    /// Como `read_until`, mas com um limite opcional próprio: ao atingi-lo
    /// devolve o que já foi lido em vez de falhar.
    fn read_until_or(
        &mut self,
        done: impl Fn(&str) -> bool,
        limit: Option<Instant>,
    ) -> Result<String, String> {
        loop {
            let text = String::from_utf8_lossy(&self.buf).into_owned();
            if done(&text) || self.eof {
                if std::env::var_os("ASMVIZ_DEBUG").is_some() {
                    eprintln!("jdb >> {text:?}");
                }
                self.buf.clear();
                return Ok(text);
            }
            let end = limit.map(|l| l.min(self.deadline)).unwrap_or(self.deadline);
            let rem = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(rem) {
                Ok(chunk) => self.buf.extend(chunk),
                Err(RecvTimeoutError::Timeout)
                    if limit.is_some() && Instant::now() < self.deadline =>
                {
                    let text = String::from_utf8_lossy(&self.buf).into_owned();
                    self.buf.clear();
                    return Ok(text);
                }
                Err(RecvTimeoutError::Timeout) => return Err("tempo limite do jdb esgotado".into()),
                Err(RecvTimeoutError::Disconnected) => self.eof = true,
            }
        }
    }

    /// Comando que não retoma a JVM: espera o próximo prompt "thread[n] ".
    fn query(&mut self, c: &str) -> Result<String, String> {
        self.send(c)?;
        let out = self.read_until(ends_with_prompt)?;
        let trimmed = rx!(r"[^\s\[\]>]+\[\d+\] $").replace(&out, "");
        Ok(trimmed.trim_end().to_string())
    }

    /// Espera um evento (passo concluído, breakpoint, exceção) ou o fim do programa.
    fn wait_event(&mut self) -> Result<Event, String> {
        let slice = self.deadline.saturating_duration_since(Instant::now());
        self.wait_event_for(slice)?
            .ok_or_else(|| "tempo limite do jdb esgotado".to_string())
    }

    /// Como `wait_event`, mas desiste após `slice` devolvendo `None`; a saída
    /// parcial fica no buffer para a próxima chamada.
    fn wait_event_for(&mut self, slice: Duration) -> Result<Option<Event>, String> {
        let ev_re = rx!(
            r#"(Step completed|Breakpoint hit|Exception occurred): .*?"thread=[^"]*", ([^\s(]+)\(\), line=(-?[\d.,]+) bci=([\d.,]+)"#
        );
        let exit_re = rx!(r"The application exited|The application has been disconnected");
        let done = |s: &str| {
            if exit_re.is_match(s) {
                return true;
            }
            match ev_re.find_iter(s).last() {
                Some(m) => ends_with_prompt(&s[m.end()..]),
                None => false,
            }
        };
        let end = Instant::now() + slice;
        let text = loop {
            let text = String::from_utf8_lossy(&self.buf).into_owned();
            if done(&text) || self.eof {
                if std::env::var_os("ASMVIZ_DEBUG").is_some() {
                    eprintln!("jdb >> {text:?}");
                }
                self.buf.clear();
                break text;
            }
            let now = Instant::now();
            if now >= self.deadline {
                return Err("tempo limite do jdb esgotado".into());
            }
            match self
                .rx
                .recv_timeout(end.min(self.deadline).saturating_duration_since(now))
            {
                Ok(chunk) => self.buf.extend(chunk),
                Err(RecvTimeoutError::Timeout) if Instant::now() < self.deadline => {
                    return Ok(None)
                }
                Err(RecvTimeoutError::Timeout) => return Err("tempo limite do jdb esgotado".into()),
                Err(RecvTimeoutError::Disconnected) => self.eof = true,
            }
        };
        if let Some(c) = ev_re.captures_iter(&text).last() {
            let full = c[2].to_string();
            let (class, method) = match full.rfind('.') {
                Some(i) => (full[..i].to_string(), full[i + 1..].to_string()),
                None => (String::new(), full),
            };
            return Ok(Some(Event::Stop {
                kind: c[1].to_string(),
                class,
                method,
                line: num(&c[3]),
                bci: num(&c[4]),
            }));
        }
        Ok(Some(Event::Exit))
    }
}

impl Drop for Jdb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Trace
// ---------------------------------------------------------------------------
struct Frame {
    class: String,
    method: String,
    line: Option<u64>,
}

fn parse_where(text: &str) -> Vec<Frame> {
    text.lines()
        .filter_map(|l| {
            let c = rx!(r"^\s*\[\d+\] (\S+) \(([^)]*)\)").captures(l)?;
            let full = c[1].to_string();
            let i = full.rfind('.')?;
            let line = c[2].rsplit_once(':').map(|(_, n)| num(n));
            Some(Frame {
                class: full[..i].to_string(),
                method: full[i + 1..].to_string(),
                line,
            })
        })
        .collect()
}

/// "nome = valor" do comando locals; devolve (nome, valor, é_argumento).
fn parse_locals(text: &str) -> (Vec<(String, String, bool)>, Option<String>) {
    let mut out = Vec::new();
    let mut arg = false;
    let mut note = None;
    for l in text.lines() {
        let t = l.trim_end();
        if t.starts_with("Method arguments:") {
            arg = true;
        } else if t.starts_with("Local variables:") {
            arg = false;
        } else if t.contains("Local variable information not available") {
            note = Some("informação de variáveis locais indisponível (compile com -g)".to_string());
        } else if let Some(c) = rx!(r"^([\w$]+) = (.*)$").captures(t) {
            out.push((c[1].to_string(), c[2].to_string(), arg));
        }
    }
    (out, note)
}

/// "instance of Tipo(id=N)" → (tipo, id)
fn object_ref(v: &str) -> Option<(String, u64)> {
    let c = rx!(r"instance of ([^\s(]+(?:\[\d*\])?) ?\(id=(\d+)\)").captures(v)?;
    Some((c[1].to_string(), c[2].parse().ok()?))
}

/// Saída do "dump x" → texto compacto em uma linha.
fn compact_dump(text: &str, name: &str) -> String {
    let body = match text.find(&format!("{name} = ")) {
        Some(i) => &text[i + name.len() + 3..],
        None => text,
    };
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let s = if lines.len() == 2 && lines[0] == "{" && lines[1] == "}" {
        "{}".to_string()
    } else if lines.len() >= 2 && lines[0] == "{" && lines[lines.len() - 1] == "}" {
        format!("{{ {} }}", lines[1..lines.len() - 1].join(", "))
    } else {
        lines.join(" ")
    };
    cap(&s, 400)
}

struct JTracer<'a> {
    j: Jdb,
    user: &'a HashSet<String>,
    statics: &'a [StaticField],
    lvts: &'a HashMap<String, Vec<LocalVar>>,
    stdout_path: std::path::PathBuf,
    out_prefix: u64,
    ctl: &'a mut InputCtl,
    jvm_pid: u32,
    meta: Value,
}

impl<'a> JTracer<'a> {
    /// Espera o próximo evento do jdb; se a JVM bloquear lendo o stdin, pede a
    /// entrada ao usuário (o tempo de espera não conta para os limites).
    fn wait_event(&mut self, steps: &[Value]) -> Result<Event, String> {
        loop {
            if let Some(ev) = self.j.wait_event_for(Duration::from_millis(150))? {
                return Ok(ev);
            }
            if blocked_on_stdin(self.jvm_pid) {
                let full = read_text(&self.stdout_path, 200_000 + self.out_prefix as usize);
                let out = full
                    .get(self.out_prefix as usize..)
                    .unwrap_or("")
                    .to_string();
                let d = self.ctl.request_input(steps, out, &self.meta)?;
                self.j.deadline += d;
            }
        }
    }

    fn heap_entry(
        &mut self,
        expr: &str,
        from: &str,
        value: &str,
        heap: &mut Vec<Value>,
        seen: &mut HashSet<u64>,
    ) -> Result<(), String> {
        let Some((ty, id)) = object_ref(value) else {
            return Ok(());
        };
        if !seen.insert(id) {
            return Ok(());
        }
        let big = rx!(r"\[(\d+)\]")
            .captures(&ty)
            .map(|c| num(&c[1]) as usize > MAX_DUMP_ARRAY)
            .unwrap_or(false);
        let content = if big {
            format!("(vetor com mais de {MAX_DUMP_ARRAY} elementos — conteúdo omitido)")
        } else {
            let d = self.j.query(&format!("dump {expr}"))?;
            if d.contains("Exception") {
                String::new()
            } else {
                compact_dump(&d, expr)
            }
        };
        heap.push(json!({"id": id, "type": ty, "from": from, "value": content}));
        Ok(())
    }

    fn record(&mut self, class: &str, method: &str, line: u64, bci: u64) -> Result<Value, String> {
        let frames = parse_where(&self.j.query("where")?);
        let mut out_frames = Vec::new();
        let mut heap = Vec::new();
        let mut seen = HashSet::new();
        let mut depth = 0usize;
        let user_idx: Vec<usize> = frames
            .iter()
            .enumerate()
            .filter(|(_, f)| self.user.contains(&f.class))
            .map(|(i, _)| i)
            .take(MAX_JAVA_FRAMES)
            .collect();
        for (n, &i) in user_idx.iter().enumerate() {
            if i > depth {
                self.j.query(&format!("up {}", i - depth))?;
                depth = i;
            }
            let f = &frames[i];
            let key = format!("{}.{}", f.class, f.method);
            let (locals, note) = parse_locals(&self.j.query("locals")?);
            let mut vars = Vec::new();
            // Métodos de instância têm "this" no slot 0. Usa "dump" (e não "print",
            // que chamaria toString() dentro do programa).
            let has_this = self
                .lvts
                .get(&key)
                .map(|l| l.iter().any(|v| v.name == "this"))
                .unwrap_or(false);
            if has_this {
                let d = self.j.query("dump this")?;
                if !d.contains("Exception") && d.contains("this = ") {
                    vars.push(json!({
                        "name": "this", "value": compact_dump(&d, "this"), "arg": true,
                        "type": f.class.rsplit('.').next().unwrap_or(""), "slot": 0, "object": true,
                    }));
                }
            }
            for (name, value, arg) in locals {
                let lv = self.lvts.get(&key).and_then(|list| {
                    let mut it = list.iter().filter(|v| v.name == name);
                    if n == 0 {
                        it.find(|v| (bci as u32) >= v.start && (bci as u32) < v.start + v.len)
                            .or_else(|| list.iter().find(|v| v.name == name))
                    } else {
                        it.next()
                    }
                });
                self.heap_entry(&name, &name, &value, &mut heap, &mut seen)?;
                vars.push(json!({
                    "name": name, "value": cap(&value, 240), "arg": arg,
                    "type": lv.map(|v| descriptor_to_type(&v.sig)),
                    "slot": lv.map(|v| v.slot),
                }));
            }
            out_frames.push(json!({
                "func": key, "line": if n == 0 { Some(line) } else { f.line },
                "bci": if n == 0 { Some(bci) } else { None }, "vars": vars, "note": note,
            }));
        }
        if depth > 0 {
            self.j.query(&format!("down {depth}"))?;
        }

        let mut statics = Vec::new();
        for s in self.statics {
            let expr = format!("{}.{}", s.class, s.name);
            let r = self.j.query(&format!("print {expr}"))?;
            let value = if r.contains("Exception") || r.contains("not loaded") {
                "(classe ainda não carregada)".to_string()
            } else {
                r.split_once(" = ")
                    .map(|(_, v)| v.trim().to_string())
                    .unwrap_or(r.clone())
            };
            self.heap_entry(&expr, &expr, &value, &mut heap, &mut seen)?;
            statics.push(
                json!({"class": s.class, "name": s.name, "type": s.ty, "value": cap(&value, 240)}),
            );
        }

        Ok(json!({
            "line": line, "func": format!("{class}.{method}"), "bci": bci, "pc": bci,
            "frames": out_frames, "heap": heap, "statics": statics,
            "outLen": file_len(&self.stdout_path).saturating_sub(self.out_prefix),
        }))
    }
}

/// "Main$$Lambda/0x…" (ou "Main$$Lambda$14/…" em JDKs antigos): classe oculta
/// que a JVM gera para um lambda ou method reference declarado em `Main`.
fn is_user_lambda(class: &str, user: &HashSet<String>) -> bool {
    class
        .split_once("$$Lambda")
        .map(|(owner, _)| user.contains(owner))
        .unwrap_or(false)
}

fn trace(
    w: &Path,
    main_class: &str,
    user: &HashSet<String>,
    statics: &[StaticField],
    lvts: &HashMap<String, Vec<LocalVar>>,
    max_steps: usize,
    ctl: &mut InputCtl,
) -> Value {
    let cfg = config();
    let meta = json!({"kind": "jvm"});
    let fail =
        |e: String| json!({"steps": [], "status": "error", "error": e, "meta": meta, "stdout": ""});
    let deadline = Instant::now() + Duration::from_secs(TRACE_SECS);
    let port = free_port();
    let stdout_path = w.join("stdout.txt");

    let java_cmd = vec![
        cfg.java.clone(),
        format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{port}"),
        "-cp".into(),
        "classes".into(),
        main_class.to_string(),
    ];
    let jdb_cmd = vec![
        cfg.jdb.clone(),
        "-attach".into(),
        format!("127.0.0.1:{port}"),
    ];
    let commands = vec![shown(&java_cmd), shown(&jdb_cmd)];
    let fifo = match ctl.open_stdin(w) {
        Ok(f) => f,
        Err(e) => return fail(e),
    };
    // em modo append, o eco da entrada (session.rs) fica na ordem certa
    let (fin, fout) = match (
        File::open(&fifo),
        OpenOptions::new().append(true).open(&stdout_path),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return fail("erro ao abrir arquivos de entrada/saída".into()),
    };
    let ferr = match fout.try_clone() {
        Ok(f) => f,
        Err(e) => return fail(e.to_string()),
    };
    let mut jvm = match Command::new(&java_cmd[0])
        .args(&java_cmd[1..])
        .current_dir(w)
        .stdin(Stdio::from(fin))
        .stdout(Stdio::from(fout))
        .stderr(Stdio::from(ferr))
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return fail(format!("não foi possível executar {}: {e}", cfg.java)),
    };

    // A JVM avisa "Listening for transport dt_socket at address: N" — essa
    // linha não faz parte da saída do programa.
    let mut out_prefix = 0u64;
    let wait_until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < wait_until {
        let text = read_text(&stdout_path, 4096);
        if let Some(i) = text.find("Listening for transport") {
            if let Some(nl) = text[i..].find('\n') {
                out_prefix = (i + nl + 1) as u64;
                break;
            }
        }
        if let Ok(Some(_)) = jvm.try_wait() {
            break;
        }
        thread::sleep(Duration::from_millis(30));
    }

    let mut status = String::from("exited");
    let mut error: Option<String> = None;
    let mut steps: Vec<Value> = Vec::new();
    match Jdb::spawn(&cfg.jdb, port, w, deadline) {
        Err(e) => {
            status = "error".into();
            error = Some(e);
        }
        Ok(j) => {
            let mut t = JTracer {
                j,
                user,
                statics,
                lvts,
                stdout_path: stdout_path.clone(),
                out_prefix,
                ctl,
                jvm_pid: jvm.id(),
                meta: meta.clone(),
            };
            let res: Result<(), String> = (|| {
                // Comandos enviados antes de o jdb terminar de se anexar são
                // perdidos ("Nothing suspended"): espera cada confirmação.
                t.j.read_until(|s| s.contains("VM Started"))?;
                // o jdb processa o evento de início da VM de forma assíncrona e
                // termina exibindo o prompt "main[1] "
                t.j.read_until_or(
                    ends_with_prompt,
                    Some(Instant::now() + Duration::from_secs(5)),
                )?;
                t.j.send("exclude java.*,javax.*,sun.*,com.sun.*,jdk.*")?;
                t.j.send(&format!("stop in {main_class}.main"))?;
                t.j.read_until(|s| s.contains("breakpoint") || s.contains("Unable to set"))?;
                t.j.send("cont")?; // "run" não retoma uma JVM anexada
                let mut ev = t.wait_event(&steps)?;
                let start = Instant::now();
                loop {
                    let Event::Stop {
                        kind,
                        class,
                        method,
                        line,
                        bci,
                    } = ev
                    else {
                        status = "exited".into();
                        return Ok(());
                    };
                    if steps.len() >= max_steps {
                        status = "limit".into();
                        return Ok(());
                    }
                    if start.elapsed().saturating_sub(t.ctl.paused())
                        > Duration::from_secs(TRACE_SECS - 20)
                    {
                        status = "timeout".into();
                        return Ok(());
                    }
                    if is_user_lambda(&class, t.user) {
                        // classe oculta gerada para um lambda / method reference
                        // do usuário: atravessa para chegar ao método alvo
                        t.j.send("stepi")?;
                        ev = t.wait_event(&steps)?;
                        continue;
                    }
                    if !t.user.contains(&class) {
                        // parou fora do código do usuário: sai do método
                        t.j.send("step up")?;
                        ev = t.wait_event(&steps)?;
                        continue;
                    }
                    steps.push(t.record(&class, &method, line, bci)?);
                    if kind == "Exception occurred" {
                        status = "exception".into();
                        error = Some("exceção não tratada — veja a saída do programa".into());
                        // deixa a JVM imprimir o stack trace e terminar
                        t.j.send("cont")?;
                        let _ = t.j.wait_event();
                        return Ok(());
                    }
                    t.j.send("stepi")?;
                    ev = t.wait_event(&steps)?;
                }
            })();
            if let Err(e) = res {
                status = "error".into();
                error = Some(e);
            }
            if status != "exited" && status != "exception" {
                let _ = jvm.kill();
            }
            drop(t);
        }
    }

    let end = Instant::now() + Duration::from_secs(5);
    let mut exit_code = None;
    while Instant::now() < end {
        if let Ok(Some(st)) = jvm.try_wait() {
            exit_code = st.code();
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    if exit_code.is_none() {
        let _ = jvm.kill();
        let _ = jvm.wait();
    }
    let full = read_text(&stdout_path, 200_000 + out_prefix as usize);
    let stdout = full.get(out_prefix as usize..).unwrap_or("").to_string();
    json!({
        "steps": steps, "status": status, "error": error, "exitCode": exit_code,
        "meta": meta, "stdout": stdout, "commands": commands,
    })
}
