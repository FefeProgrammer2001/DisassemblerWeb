//! Cliente mínimo da interface GDB/MI (gdb --interpreter=mi3).

use serde_json::{Map, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Parser da sintaxe de saída do MI: result ("k=v,..."), c-string, tuple, list
// ---------------------------------------------------------------------------
struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn results(&mut self, end: Option<u8>) -> Map<String, Value> {
        let mut m = Map::new();
        loop {
            match self.peek() {
                None => break,
                Some(c) if Some(c) == end => break,
                Some(b',') => {
                    self.i += 1;
                    continue;
                }
                _ => {}
            }
            let start = self.i;
            while let Some(c) = self.peek() {
                if c == b'=' || c == b',' || Some(c) == end {
                    break;
                }
                self.i += 1;
            }
            let key = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
            if self.peek() == Some(b'=') {
                self.i += 1;
                let v = self.value();
                m.insert(key, v);
            }
        }
        m
    }

    fn value(&mut self) -> Value {
        match self.peek() {
            Some(b'"') => Value::String(self.cstring()),
            Some(b'{') => {
                self.i += 1;
                let m = self.results(Some(b'}'));
                self.i += 1;
                Value::Object(m)
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    match self.peek() {
                        None => break,
                        Some(b']') => {
                            self.i += 1;
                            break;
                        }
                        Some(b',') => self.i += 1,
                        Some(b'"') | Some(b'{') | Some(b'[') => items.push(self.value()),
                        _ => {
                            // lista de results (k=v): guarda só o valor
                            while let Some(c) = self.peek() {
                                if c == b'=' || c == b',' || c == b']' {
                                    break;
                                }
                                self.i += 1;
                            }
                            if self.peek() == Some(b'=') {
                                self.i += 1;
                                items.push(self.value());
                            }
                        }
                    }
                }
                Value::Array(items)
            }
            _ => Value::Null,
        }
    }

    fn cstring(&mut self) -> String {
        self.i += 1; // aspas de abertura
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.peek().unwrap_or(b'\\');
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'0'..=b'7' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.i += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        other => out.push(other),
                    }
                }
                _ => out.push(c),
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}

/// Divide "classe,resultados" e devolve (classe, objeto de resultados).
pub fn parse_record(rest: &str) -> (String, Value) {
    let (class, results) = match rest.find(',') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let mut p = P {
        s: results.as_bytes(),
        i: 0,
    };
    (class.trim().to_string(), Value::Object(p.results(None)))
}

/// Primeiro número de uma string do gdb ("0x7ffe...", "(int *) 0x...", "12", "-3").
pub fn parse_num(s: &str) -> Option<u64> {
    if let Some(i) = s.find("0x") {
        let h: String = s[i + 2..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        return u64::from_str_radix(&h, 16).ok();
    }
    let t = s.trim();
    let d: String = t
        .chars()
        .enumerate()
        .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-'))
        .map(|(_, c)| c)
        .collect();
    d.parse::<i64>().ok().map(|x| x as u64)
}

/// Aspas no formato c-string do MI.
pub fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ---------------------------------------------------------------------------
// Sessão do gdb
// ---------------------------------------------------------------------------
pub struct Gdb {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
    tok: u64,
    /// pode ser adiado enquanto o programa espera a entrada do usuário
    pub deadline: Instant,
    pub log: String,
}

impl Gdb {
    pub fn spawn(gdb: &str, cwd: &Path, deadline: Instant) -> Result<Self, String> {
        let mut child = Command::new(gdb)
            .args(["--interpreter=mi3", "-nx", "-q"])
            .current_dir(cwd)
            .env("DEBUGINFOD_URLS", "")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("não foi possível executar {gdb}: {e}"))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Gdb {
            child,
            stdin,
            rx,
            tok: 0,
            deadline,
            log: String::new(),
        })
    }

    fn next_line(&mut self) -> Result<String, String> {
        let rem = self.deadline.saturating_duration_since(Instant::now());
        match self.rx.recv_timeout(rem) {
            Ok(l) => Ok(l),
            Err(RecvTimeoutError::Timeout) => Err("tempo limite do gdb esgotado".into()),
            Err(RecvTimeoutError::Disconnected) => Err("o gdb terminou inesperadamente".into()),
        }
    }

    fn note(&mut self, line: &str) {
        if let Some(s) = line.strip_prefix('~').or_else(|| line.strip_prefix('&')) {
            let mut p = P {
                s: s.as_bytes(),
                i: 0,
            };
            let text = if s.starts_with('"') {
                p.cstring()
            } else {
                s.to_string()
            };
            if self.log.len() < 20_000 {
                self.log.push_str(&text);
            }
        }
    }

    /// Envia um comando MI e espera pelo registro de resultado (^done, ^running, ^error...).
    pub fn cmd(&mut self, c: &str) -> Result<Value, String> {
        self.tok += 1;
        let tok = self.tok;
        writeln!(self.stdin, "{tok}{c}").map_err(|e| format!("falha ao escrever no gdb: {e}"))?;
        let prefix = format!("{tok}^");
        loop {
            let line = self.next_line()?;
            if let Some(rest) = line.strip_prefix(&prefix) {
                let (class, v) = parse_record(rest);
                if class == "error" {
                    return Err(v["msg"].as_str().unwrap_or("erro").to_string());
                }
                return Ok(v);
            }
            self.note(&line);
        }
    }

    pub fn console(&mut self, c: &str) -> Result<Value, String> {
        self.cmd(&format!("-interpreter-exec console {}", q(c)))
    }

    /// Espera o próximo registro assíncrono *stopped.
    pub fn wait_stopped(&mut self) -> Result<Value, String> {
        loop {
            let line = self.next_line()?;
            if let Some(rest) = line.strip_prefix("*stopped") {
                let rest = rest.strip_prefix(',').unwrap_or(rest);
                return Ok(parse_record(&format!("stopped,{rest}")).1);
            }
            self.note(&line);
        }
    }

    /// Como `wait_stopped`, mas desiste após `slice` devolvendo `None` (o gdb
    /// continua esperando o programa parar).
    pub fn wait_stopped_for(&mut self, slice: Duration) -> Result<Option<Value>, String> {
        let end = Instant::now() + slice;
        loop {
            let now = Instant::now();
            if now >= self.deadline {
                return Err("tempo limite do gdb esgotado".into());
            }
            let line = match self
                .rx
                .recv_timeout(end.min(self.deadline).saturating_duration_since(now))
            {
                Ok(l) => l,
                Err(RecvTimeoutError::Timeout) if Instant::now() < self.deadline => {
                    return Ok(None)
                }
                Err(RecvTimeoutError::Timeout) => return Err("tempo limite do gdb esgotado".into()),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("o gdb terminou inesperadamente".into())
                }
            };
            if let Some(rest) = line.strip_prefix("*stopped") {
                let rest = rest.strip_prefix(',').unwrap_or(rest);
                return Ok(Some(parse_record(&format!("stopped,{rest}")).1));
            }
            self.note(&line);
        }
    }

    /// Avalia uma expressão no frame `level`.
    pub fn eval(&mut self, level: usize, expr: &str) -> Result<String, String> {
        let v = self.cmd(&format!(
            "-data-evaluate-expression --thread 1 --frame {level} {}",
            q(expr)
        ))?;
        Ok(v["value"].as_str().unwrap_or("").to_string())
    }

    pub fn read_hex(&mut self, addr: u64, len: u64) -> Option<String> {
        if len == 0 {
            return Some(String::new());
        }
        let v = self
            .cmd(&format!("-data-read-memory-bytes {addr:#x} {len}"))
            .ok()?;
        let mem = v["memory"].as_array()?;
        let s: String = mem.iter().filter_map(|m| m["contents"].as_str()).collect();
        (!s.is_empty()).then_some(s)
    }
}

impl Drop for Gdb {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "-gdb-exit");
        thread::sleep(std::time::Duration::from_millis(30));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
