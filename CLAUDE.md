# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`asmviz`, deployed at https://www.mydisassembler.net/ on a single lightweight Hostinger server: a web app (Portuguese UI and docs) where the user types C, C++, Rust or Java; the server compiles it, shows the generated assembly/bytecode, and single-steps execution instruction by instruction while the UI shows registers, stack frames, heap, globals and program output. `README.md` has the full tool/command reference (it is the source of truth for the exact compiler/gdb/jdb invocations).

## Commands

```sh
cargo run --release          # serves on 0.0.0.0:36476 (HOST, PORT, MAX_JOBS, STATIC_DIR, tool path env vars; see README)
cargo build --release
docker compose up -d --build # sandboxed deployment with all toolchains
```

There are no tests, no linter config and no frontend build step. `static/` is plain HTML/JS served as-is by the Rust server, so frontend edits only need a browser reload.

Runtime needs external tools on PATH: `clang`, `lld`, `llvm-objdump`, `llvm-nm`, `gdb`, `rustc`, `javac`/`javap`/`java`/`jdb`, plus `qemu-aarch64` and an ARM64 sysroot for ARM64 targets. `GET /api/tools` reports which are available.

## Architecture

Rust backend (`tiny_http`, `serde_json`, `regex`; no async, no Python) plus a static frontend. Routes are in `src/main.rs` (`/api/tools`, `/api/build`, `/api/input`).

- `native.rs`: C/C++ pipeline. Runs clang (`-S` for assembly, then links a static binary with `nobuf.c` to unbuffer stdout), `llvm-objdump -l` for disassembly with source-line mapping, and drives gdb over GDB/MI with `stepi` from `main`, building one trace step per instruction (registers, stack frames/variables, heap reached via pointers, globals).
- `rust.rs`: thin layer over `native.rs`. Compiles with `rustc` and reuses the objdump parser and gdb trace code, starting from `prog::main`.
- `mi.rs`: GDB/MI client and output parser.
- `java.rs`: `javac -g`, `javap -c` for bytecode, and a trace via `jdb` attached over JDWP, parsed from its text output (`ASMVIZ_DEBUG` logs the conversation).
- `session.rs` + `util.rs`: interactive stdin. The program's stdin is a FIFO whose write end the server holds. While the trace thread waits, the server inspects `/proc/<pid>/task/*/syscall`; if a thread is blocked in `read` on fd 0, it returns the steps so far with status `input`. `POST /api/input` then writes to the FIFO and the trace resumes. `util.rs` also has timeout-bounded process spawning and temp dirs. `MAX_JOBS` bounds concurrent builds.
- `static/app.js` / `index.html`: the UI (code editor, assembly/source cross-highlighting, step navigation, memory views). It consumes the trace JSON produced by the backend, so changing a trace field means editing both a Rust module and `app.js`.

Library calls (`printf`, `malloc`, std code) are executed in one go rather than stepped into. Several design choices follow from the sandbox: binaries are static and non-PIE so code addresses are stable, and the Docker container runs read-only with `/tmp` mounted `exec`, because compiled programs run from there. The server executes arbitrary user code, so keep those isolation constraints intact.

## Repo notes

- `.claude/settings.json` has a PostToolUse hook that formats edited files: `rustfmt` for `.rs`, `prettier` (via `bunx`) for `.js/.html/.css/.json`. The existing code was never auto-formatted, so the first edit to a file reformats all of it.
- Production is one small server, so keep builds, memory and concurrency (`MAX_JOBS`) modest.

- `compiler/` is untracked and contains only build output (`target/`) and a `Cargo.lock`; its sources are not in the repo, even though commit `78fd105` mentions a C compiler / CPU stepper for WebAssembly. Don't assume it is part of the build.
