//! Servidor do visualizador C/C++/Rust → Assembly e Java → bytecode.
//!
//! - Serve os arquivos estáticos de `static/` (index.html, app.js).
//! - POST /api/build  compila (e opcionalmente executa passo a passo) o código.
//! - POST /api/input  envia a entrada digitada a uma execução pausada (session.rs).
//! - GET  /api/tools  informa quais ferramentas estão disponíveis.
//!
//! ATENÇÃO: o servidor compila e executa qualquer código recebido. Por padrão
//! ele escuta em todas as interfaces (0.0.0.0) para testes na rede local.

mod java;
mod mi;
mod native;
mod rust;
mod session;
mod util;

use serde_json::{json, Value};
use std::io::Read;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::thread;
use tiny_http::{Header, Method, Request, Response, Server};

const MAX_BODY: usize = 1 << 20;

pub struct Config {
    pub clang: String,
    pub clangxx: String,
    pub objdump: String,
    pub nm: String,
    pub cxxfilt: String,
    pub gdb: String,
    pub sysroot: String,
    pub qemu: Option<String>,
    pub javac: String,
    pub javap: String,
    pub java: String,
    pub jdb: String,
    pub rustc: String,
    pub static_dir: PathBuf,
    pub max_jobs: usize,
}

pub fn config() -> &'static Config {
    static CFG: OnceLock<Config> = OnceLock::new();
    CFG.get_or_init(|| {
        let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        Config {
            clang: env("CLANG", "clang"),
            clangxx: env("CLANGXX", "clang++"),
            objdump: env("LLVM_OBJDUMP", "llvm-objdump"),
            nm: env("LLVM_NM", "llvm-nm"),
            cxxfilt: env("LLVM_CXXFILT", "llvm-cxxfilt"),
            gdb: env("GDB", "gdb"),
            sysroot: env("AARCH64_SYSROOT", "/usr/aarch64-linux-gnu"),
            qemu: std::env::var("QEMU_AARCH64")
                .ok()
                .or_else(|| util::which("qemu-aarch64"))
                .or_else(|| util::which("qemu-aarch64-static")),
            javac: env("JAVAC", "javac"),
            javap: env("JAVAP", "javap"),
            java: env("JAVA", "java"),
            jdb: env("JDB", "jdb"),
            rustc: env("RUSTC", "rustc"),
            static_dir: std::env::var("STATIC_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/static"))),
            max_jobs: env("MAX_JOBS", "4").parse().unwrap_or(4),
        }
    })
}

fn tools() -> Value {
    let c = config();
    let gcc_arm = std::fs::read_dir("/usr/lib/gcc/aarch64-linux-gnu")
        .ok()
        .and_then(|mut d| d.next())
        .and_then(|e| e.ok())
        .map(|e| e.path().to_string_lossy().into_owned());
    let sysroot_ok = std::path::Path::new(&c.sysroot)
        .join("include/stdio.h")
        .exists();
    json!({
        "clang": util::which(&c.clang),
        "clang++": util::which(&c.clangxx),
        "lld": util::which("ld.lld"),
        "objdump": util::which(&c.objdump),
        "gdb": util::which(&c.gdb),
        "qemu": c.qemu,
        "aarch64Sysroot": sysroot_ok.then(|| c.sysroot.clone()),
        "aarch64Gcc": gcc_arm,
        "javac": util::which(&c.javac),
        "jdb": util::which(&c.jdb),
        "rustc": util::which(&c.rustc),
    })
}

fn json_response(v: &Value, code: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(serde_json::to_vec(v).unwrap_or_default())
        .with_status_code(code)
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

struct JobGuard;
impl Drop for JobGuard {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

fn read_json(req: &mut Request) -> Result<Value, Response<std::io::Cursor<Vec<u8>>>> {
    let mut body = Vec::new();
    if req
        .as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut body)
        .is_err()
        || body.len() > MAX_BODY
    {
        return Err(json_response(
            &json!({"ok": false, "diagnostics": "requisição muito grande ou inválida"}),
            413,
        ));
    }
    serde_json::from_slice::<Value>(&body)
        .map_err(|_| json_response(&json!({"ok": false, "diagnostics": "JSON inválido"}), 400))
}

fn build(r: &Value, ctl: Option<&mut session::InputCtl>) -> Value {
    match r["lang"].as_str().unwrap_or("c") {
        "c" => native::build(r, &native::C, ctl),
        "cpp" => native::build(r, &native::CPP, ctl),
        "rust" => rust::build(r, ctl),
        "java" => java::build(r, ctl),
        other => json!({"ok": false, "diagnostics": format!("linguagem não suportada: {other}")}),
    }
}

fn handle_build(req: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let r = match read_json(req) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    if ACTIVE.fetch_add(1, Ordering::SeqCst) >= config().max_jobs {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
        return json_response(
            &json!({"ok": false, "diagnostics": "servidor ocupado; tente novamente em instantes"}),
            503,
        );
    }
    let guard = JobGuard;
    if !r["trace"].as_bool().unwrap_or(false) {
        return json_response(&build(&r, None), 200);
    }
    // execução numa thread própria, que pode pausar à espera de entrada; a vaga
    // em MAX_JOBS fica ocupada até ela terminar
    let stdin = r["stdin"].as_str().unwrap_or("").to_string();
    let res = session::start(stdin, move |ctl| {
        let _guard = guard;
        build(&r, Some(ctl))
    });
    json_response(&res, 200)
}

fn handle_static(path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let rel = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if rel.split('/').any(|p| p == ".." || p.starts_with('.')) {
        return Response::from_string("proibido").with_status_code(403);
    }
    match std::fs::read(config().static_dir.join(rel)) {
        Ok(data) => Response::from_data(data)
            .with_header(Header::from_bytes("Content-Type", mime(rel)).unwrap()),
        Err(_) => Response::from_string("não encontrado").with_status_code(404),
    }
}

fn handle(mut req: Request) {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();
    let method = req.method().clone();
    let resp = match (&method, path.as_str()) {
        (Method::Get, "/api/tools") => json_response(&tools(), 200),
        (Method::Post, "/api/build") => {
            println!(
                "{} POST /api/build",
                req.remote_addr().map(|a| a.to_string()).unwrap_or_default()
            );
            handle_build(&mut req)
        }
        (Method::Post, "/api/input") => match read_json(&mut req) {
            Ok(r) => json_response(&session::input(&r), 200),
            Err(resp) => resp,
        },
        (Method::Get, p) | (Method::Head, p) => handle_static(p),
        _ => Response::from_string("método não permitido").with_status_code(405),
    };
    let _ = req.respond(resp);
}

/// IP da interface usada para a rede local (não envia pacotes).
fn lan_ip() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

fn main() {
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(36476);
    let server = match Server::http((host.as_str(), port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("não foi possível escutar em {host}:{port}: {e}");
            std::process::exit(1);
        }
    };
    println!("Visualizador C/C++/Rust/Java escutando em {host}:{port}");
    println!("  local: http://127.0.0.1:{port}");
    if host == "0.0.0.0" || host == "::" {
        if let Some(ip) = lan_ip() {
            println!("  rede:  http://{ip}:{port}");
        }
        println!("  AVISO: qualquer máquina com acesso a esta porta pode compilar e executar código aqui.");
    }
    println!("  arquivos estáticos: {}", config().static_dir.display());
    for req in server.incoming_requests() {
        thread::spawn(move || handle(req));
    }
}
