//! Utilidades: execução de processos com tempo limite, diretório temporário, etc.

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// Executa `cmd` em `cwd`, capturando stdout/stderr, e mata o processo após `timeout`.
pub fn run(cmd: &[String], cwd: &Path, timeout: Duration) -> Output {
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            return Output {
                code: 127,
                stdout: String::new(),
                stderr: format!("não foi possível executar {}: {e}\n", cmd[0]),
            }
        }
    };
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t1 = thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        b
    });
    let t2 = thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(-1),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break 124;
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => break -1,
        }
    };
    let stdout = String::from_utf8_lossy(&t1.join().unwrap_or_default()).into_owned();
    let mut stderr = String::from_utf8_lossy(&t2.join().unwrap_or_default()).into_owned();
    if code == 124 {
        stderr.push_str(&format!("\ntempo esgotado ({}s): {}\n", timeout.as_secs(), cmd[0]));
    }
    Output { code, stdout, stderr }
}

/// Linha de comando legível (com aspas simples quando necessário).
pub fn shown(cmd: &[String]) -> String {
    cmd.iter()
        .map(|a| {
            if !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "-_=./,:+@%".contains(c)) {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn sv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Diretório temporário removido automaticamente.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> std::io::Result<Self> {
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "asmviz-{}-{}-{:x}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed),
            nanos
        ));
        std::fs::create_dir_all(&p)?;
        Ok(TempDir(p))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(45123)
}

/// Procura um executável no PATH.
pub fn which(name: &str) -> Option<String> {
    if name.contains('/') {
        return Path::new(name).exists().then(|| name.to_string());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Lê no máximo `max` bytes de um arquivo como texto.
pub fn read_text(path: &Path, max: usize) -> String {
    match std::fs::read(path) {
        Ok(mut b) => {
            b.truncate(max);
            String::from_utf8_lossy(&b).into_owned()
        }
        Err(_) => String::new(),
    }
}

pub fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub fn cap(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(3)).collect();
        t.push_str("...");
        t
    }
}

/// Cria um FIFO (pipe nomeado) com o `mkfifo` do sistema.
pub fn mkfifo(path: &Path) -> Result<(), String> {
    let out = Command::new("mkfifo")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("não foi possível executar mkfifo: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("mkfifo falhou: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Verdadeiro se alguma thread do processo está bloqueada numa leitura do fd 0
/// (stdin). Lê /proc/<pid>/task/*/syscall: "<nr> <arg0> ..." enquanto a thread
/// está dentro de uma chamada de sistema. Vale para o programa nativo, para o
/// qemu-user (que faz o read no host em nome do programa ARM64) e para a JVM.
pub fn blocked_on_stdin(pid: u32) -> bool {
    // read / readv / pread64 da arquitetura do host
    const READS: &[u64] = if cfg!(target_arch = "aarch64") { &[63, 65, 67] } else { &[0, 19, 17] };
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else { return false };
    tasks.flatten().any(|t| {
        let Ok(s) = std::fs::read_to_string(t.path().join("syscall")) else { return false };
        let mut f = s.split_whitespace();
        let nr = f.next().and_then(|n| n.parse::<u64>().ok());
        let fd = f.next().and_then(|a| u64::from_str_radix(a.trim_start_matches("0x"), 16).ok());
        matches!((nr, fd), (Some(n), Some(0)) if READS.contains(&n))
    })
}
