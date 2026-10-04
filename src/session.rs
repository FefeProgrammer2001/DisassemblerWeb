//! Entrada interativa durante o trace.
//!
//! O trace roda numa thread própria. O stdin do programa é um FIFO cuja ponta
//! de escrita fica com o servidor. Quando o programa bloqueia lendo o stdin
//! (detectado por `blocked_on_stdin`), o tracer chama `InputCtl::request_input`:
//! os passos gravados até ali vão para o navegador com status "input" e a
//! thread espera o texto digitado (POST /api/input), que é escrito no FIFO e
//! ecoado na saída do programa, como num terminal.

use crate::util::mkfifo;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// Quanto tempo uma execução pausada espera o usuário digitar.
const INPUT_WAIT: Duration = Duration::from_secs(15 * 60);
/// Capacidade do pipe no Linux; o texto inicial maior que isso é cortado.
const MAX_PREFILL: usize = 60_000;

pub enum Input {
    Text(String),
    Eof,
    Cancel,
}

pub struct InputCtl {
    id: u64,
    prefill: String,
    pipe: Option<File>,
    stdout_path: Option<PathBuf>,
    updates: Sender<Value>,
    inputs: Receiver<Input>,
    base: Option<Value>,
    sent: usize,
    paused: Duration,
    paused_once: bool,
}

impl InputCtl {
    /// Resposta da compilação (asm, disasm...), enviada junto com a primeira pausa.
    pub fn set_base(&mut self, base: Value) {
        self.base = Some(base);
    }

    /// Cria `stdin.fifo` e um `stdout.txt` vazio em `dir` e escreve o texto
    /// inicial (campo "Entrada do programa"). O FIFO é aberto para leitura e
    /// escrita: assim abrir a outra ponta não bloqueia, e o programa só recebe
    /// EOF quando o usuário pedir.
    pub fn open_stdin(&mut self, dir: &Path) -> Result<PathBuf, String> {
        let fifo = dir.join("stdin.fifo");
        mkfifo(&fifo)?;
        let mut pipe = OpenOptions::new().read(true).write(true).open(&fifo).map_err(|e| format!("erro ao abrir o stdin: {e}"))?;
        let mut text = std::mem::take(&mut self.prefill);
        if text.len() > MAX_PREFILL {
            let mut cut = MAX_PREFILL;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        pipe.write_all(text.as_bytes()).map_err(|e| format!("erro ao escrever no stdin: {e}"))?;
        let out = dir.join("stdout.txt");
        File::create(&out).map_err(|e| format!("erro ao criar a saída: {e}"))?;
        self.pipe = Some(pipe);
        self.stdout_path = Some(out);
        Ok(fifo)
    }

    /// Tempo total gasto esperando o usuário (não conta para o limite do trace).
    pub fn paused(&self) -> Duration {
        self.paused
    }

    /// O programa está bloqueado lendo o stdin: envia os passos novos e espera
    /// a entrada do usuário. Devolve quanto tempo ficou parado.
    pub fn request_input(&mut self, steps: &[Value], stdout: String, meta: &Value) -> Result<Duration, String> {
        let Some(pipe) = self.pipe.as_mut() else {
            return Err("o programa está esperando entrada, mas o stdin já foi fechado".into());
        };
        let mut msg = self.base.take().unwrap_or_else(|| json!({"ok": true}));
        msg["session"] = json!(self.id);
        msg["trace"] = json!({
            "status": "input", "steps": steps.get(self.sent..).unwrap_or(&[]), "offset": self.sent,
            "stdout": stdout, "meta": meta,
        });
        self.sent = steps.len();
        self.paused_once = true;
        let t0 = Instant::now();
        self.updates.send(msg).map_err(|_| "conexão com o navegador perdida".to_string())?;
        let input = match self.inputs.recv_timeout(INPUT_WAIT) {
            Ok(i) => i,
            Err(RecvTimeoutError::Timeout) => return Err("tempo de espera pela entrada esgotado".into()),
            Err(RecvTimeoutError::Disconnected) => return Err("sessão encerrada".into()),
        };
        match input {
            Input::Text(s) => {
                pipe.write_all(s.as_bytes()).map_err(|e| format!("erro ao escrever no stdin: {e}"))?;
                // eco, como o terminal faria
                if let Some(p) = &self.stdout_path {
                    if let Ok(mut f) = OpenOptions::new().append(true).open(p) {
                        let _ = f.write_all(s.as_bytes());
                    }
                }
            }
            Input::Eof => self.pipe = None,
            Input::Cancel => return Err("execução cancelada".into()),
        }
        let d = t0.elapsed();
        self.paused += d;
        Ok(d)
    }

    /// Resposta final. Se houve pausa, o navegador já tem a compilação e os
    /// primeiros passos: manda só o restante do trace.
    fn finish(&self, mut res: Value) -> Value {
        if !self.paused_once {
            return res;
        }
        let mut t = res["trace"].take();
        if let Some(steps) = t["steps"].as_array_mut() {
            steps.drain(..self.sent.min(steps.len()));
        }
        t["offset"] = json!(self.sent);
        json!({"ok": res["ok"], "lang": res["lang"], "trace": t, "session": self.id})
    }
}

struct Handle {
    inputs: Sender<Input>,
    updates: Receiver<Value>,
}

fn sessions() -> &'static Mutex<HashMap<u64, Handle>> {
    static S: OnceLock<Mutex<HashMap<u64, Handle>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Espera a próxima mensagem da thread do trace; se for uma pausa, guarda a sessão.
fn next_update(id: u64, h: Handle) -> Value {
    match h.updates.recv() {
        Ok(v) => {
            if v["trace"]["status"] == "input" {
                sessions().lock().unwrap().insert(id, h);
            }
            v
        }
        Err(_) => json!({"ok": false, "diagnostics": "a execução terminou inesperadamente"}),
    }
}

/// Roda `job` (compilação + trace) numa thread com entrada interativa e devolve
/// a primeira resposta: o resultado final ou uma pausa à espera de entrada.
pub fn start<F>(stdin: String, job: F) -> Value
where
    F: FnOnce(&mut InputCtl) -> Value + Send + 'static,
{
    static N: AtomicU64 = AtomicU64::new(1);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let (up_tx, up_rx) = channel();
    let (in_tx, in_rx) = channel();
    thread::spawn(move || {
        let mut ctl = InputCtl {
            id,
            prefill: stdin,
            pipe: None,
            stdout_path: None,
            updates: up_tx,
            inputs: in_rx,
            base: None,
            sent: 0,
            paused: Duration::ZERO,
            paused_once: false,
        };
        let res = job(&mut ctl);
        let res = ctl.finish(res);
        sessions().lock().unwrap().remove(&id);
        let _ = ctl.updates.send(res);
    });
    next_update(id, Handle { inputs: in_tx, updates: up_rx })
}

/// POST /api/input: {session, text} | {session, eof: true} | {session, cancel: true}
pub fn input(req: &Value) -> Value {
    let id = req["session"].as_u64().unwrap_or(0);
    let Some(h) = sessions().lock().unwrap().remove(&id) else {
        return json!({"ok": false, "diagnostics": "sessão não encontrada (a execução já terminou ou expirou)"});
    };
    let input = if req["cancel"].as_bool().unwrap_or(false) {
        Input::Cancel
    } else if req["eof"].as_bool().unwrap_or(false) {
        Input::Eof
    } else {
        Input::Text(req["text"].as_str().unwrap_or("").to_string())
    };
    if h.inputs.send(input).is_err() {
        return json!({"ok": false, "diagnostics": "a execução já terminou"});
    }
    next_update(id, h)
}
