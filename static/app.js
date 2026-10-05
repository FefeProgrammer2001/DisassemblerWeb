"use strict";

// ======================================================================
//  Utilidades
// ======================================================================
const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
const esc = (s) =>
  String(s).replace(
    /[&<>"]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c],
  );
const hx = (n) => (n == null ? "?" : "0x" + n.toString(16));
const lineColor = (n, a = 1) => `hsla(${(n * 47) % 360}, 70%, 62%, ${a})`;
const store = {
  get(k, d) {
    try {
      const v = localStorage.getItem(k);
      return v == null ? d : v;
    } catch {
      return d;
    }
  },
  set(k, v) {
    try {
      localStorage.setItem(k, v);
    } catch {
      /* sem storage */
    }
  },
};

// ======================================================================
//  Exemplos por linguagem
// ======================================================================
const EXAMPLES = {
  c: {
    "Variáveis e aritmética": `#include <stdio.h>

int main(void) {
    int a = 5;
    int b = 7;
    int c = a * b + 2;
    printf("c = %d\\n", c);
    return 0;
}
`,
    "Laço e vetor": `#include <stdio.h>

int main(void) {
    int v[5] = {3, 1, 4, 1, 5};
    int soma = 0;
    for (int i = 0; i < 5; i++) {
        soma += v[i];
    }
    printf("soma = %d\\n", soma);
    return 0;
}
`,
    "Recursão (fatorial)": `#include <stdio.h>

long fatorial(int n) {
    if (n <= 1)
        return 1;
    return n * fatorial(n - 1);
}

int main(void) {
    long r = fatorial(4);
    printf("4! = %ld\\n", r);
    return 0;
}
`,
    "Ponteiros e malloc": `#include <stdio.h>
#include <stdlib.h>

void troca(int *x, int *y) {
    int t = *x;
    *x = *y;
    *y = t;
}

int main(void) {
    int a = 10, b = 20;
    troca(&a, &b);
    int *p = malloc(4 * sizeof(int));
    for (int i = 0; i < 4; i++)
        p[i] = i * i;
    printf("a=%d b=%d p[3]=%d\\n", a, b, p[3]);
    free(p);
    return 0;
}
`,
    "Globais e struct": `#include <stdio.h>

struct Ponto {
    int x, y;
};

int contador = 0;
static const char *msg = "ola";

void incrementa(struct Ponto *p) {
    p->x++;
    p->y += 2;
    contador++;
}

int main(void) {
    struct Ponto pt = {1, 2};
    for (int i = 0; i < 3; i++)
        incrementa(&pt);
    printf("%s (%d, %d) contador=%d\\n", msg, pt.x, pt.y, contador);
    return 0;
}
`,
    "Leitura com scanf": `#include <stdio.h>

int main(void) {
    int n, soma = 0;
    scanf("%d", &n);
    for (int i = 1; i <= n; i++)
        soma += i;
    printf("1 + ... + %d = %d\\n", n, soma);
    return 0;
}
`,
  },
  cpp: {
    "Classe e métodos": `#include <iostream>

class Contador {
public:
    int valor = 0;
    void incrementa(int passo) { valor += passo; }
};

int main() {
    Contador c;
    for (int i = 1; i <= 3; i++)
        c.incrementa(i);
    std::cout << "valor = " << c.valor << std::endl;
    return 0;
}
`,
    "Referências e new/delete": `#include <cstdio>

void dobra(int &x) {
    x *= 2;
}

int main() {
    int a = 21;
    dobra(a);
    int *v = new int[4]{1, 2, 3, 4};
    v[2] = a;
    std::printf("a=%d v[2]=%d\\n", a, v[2]);
    delete[] v;
    return 0;
}
`,
    Templates: `#include <cstdio>

template <typename T>
T maximo(T a, T b) {
    return a > b ? a : b;
}

int main() {
    int x = maximo(3, 7);
    double y = maximo(2.5, 1.5);
    std::printf("%d %.1f\\n", x, y);
    return 0;
}
`,
    "Struct e ponteiro para membro": `#include <cstdio>

struct Retangulo {
    int largura, altura;
    int area() const { return largura * altura; }
};

int main() {
    Retangulo r{3, 4};
    Retangulo *p = &r;
    p->largura = 5;
    std::printf("area = %d\\n", p->area());
    return 0;
}
`,
  },
  rust: {
    "Struct e métodos": `struct Conta {
    saldo: i64,
}

impl Conta {
    fn deposita(&mut self, valor: i64) {
        self.saldo += valor;
    }
}

fn main() {
    let mut c = Conta { saldo: 100 };
    for v in [10, 20, 30] {
        c.deposita(v);
    }
    println!("saldo = {}", c.saldo);
}
`,
    "Vec, Box e referências": `fn dobra(x: &mut i32) {
    *x *= 2;
}

fn main() {
    let mut a = 21;
    dobra(&mut a);
    let b = Box::new(a + 1);
    let mut v = vec![1, 2, 3];
    v.push(*b);
    let soma: i32 = v.iter().sum();
    println!("a={a} b={b} soma={soma}");
}
`,
    "Recursão e static": `static mut CHAMADAS: u32 = 0;

fn fatorial(n: u64) -> u64 {
    unsafe { CHAMADAS += 1; }
    if n <= 1 {
        return 1;
    }
    n * fatorial(n - 1)
}

fn main() {
    let r = fatorial(4);
    let chamadas = unsafe { CHAMADAS };
    println!("4! = {r} ({chamadas} chamadas)");
}
`,
    "Leitura da entrada": `use std::io;

fn main() {
    let mut linha = String::new();
    io::stdin().read_line(&mut linha).unwrap();
    let n: u32 = linha.trim().parse().unwrap();
    let mut soma = 0;
    for i in 1..=n {
        soma += i;
    }
    println!("1 + ... + {n} = {soma}");
}
`,
  },
  java: {
    "Variáveis e métodos": `public class Main {
    static int dobro(int x) {
        return x * 2;
    }

    public static void main(String[] args) {
        int a = 5;
        int b = dobro(a);
        int c = a + b;
        System.out.println("c = " + c);
    }
}
`,
    "Objetos e heap": `public class Main {
    static class Ponto {
        int x, y;

        Ponto(int x, int y) {
            this.x = x;
            this.y = y;
        }

        void move(int dx) {
            x += dx;
        }
    }

    public static void main(String[] args) {
        Ponto p = new Ponto(1, 2);
        p.move(5);
        int[] v = {10, 20, 30};
        v[1] = p.x;
        System.out.println(p.x + " " + v[1]);
    }
}
`,
    "Recursão e static": `public class Main {
    static int chamadas = 0;

    static long fatorial(int n) {
        chamadas++;
        if (n <= 1)
            return 1;
        return n * fatorial(n - 1);
    }

    public static void main(String[] args) {
        long r = fatorial(4);
        System.out.println("4! = " + r + " (" + chamadas + " chamadas)");
    }
}
`,
    "Laço e vetor": `public class Main {
    public static void main(String[] args) {
        int[] v = {3, 1, 4, 1, 5};
        int soma = 0;
        for (int i = 0; i < v.length; i++) {
            soma += v[i];
        }
        System.out.println("soma = " + soma);
    }
}
`,
  },
};
const DEFAULT_EXAMPLE = {
  c: "Ponteiros e malloc",
  cpp: "Classe e métodos",
  rust: "Struct e métodos",
  java: "Objetos e heap",
};
const LANG_NAME = { c: "C", cpp: "C++", rust: "Rust", java: "Java" };
// aba com a saída de assembly do compilador
const S_TAB = { c: "clang -S", cpp: "clang -S", rust: "rustc --emit asm" };

// ======================================================================
//  Estado
// ======================================================================
const S = {
  lang: "c",
  result: null, // resposta de /api/build
  steps: [], // trace.steps
  meta: null, // trace.meta
  idx: 0,
  asmTab: "disasm",
  memTab: "map",
  timer: null,
  hoverLine: null,
  busy: false,
  session: null, // execução pausada esperando entrada (POST /api/input)
};
const isJava = () => S.lang === "java";
const resultIsJava = () => S.result && S.result.lang === "java";

const el = {
  src: $("#src"),
  gutter: $("#gutter"),
  editor: $("#editor"),
  codeView: $("#codeView"),
  btnEdit: $("#btnEdit"),
  stdin: $("#stdin"),
  lang: $("#lang"),
  arch: $("#arch"),
  archWrap: $("#archWrap"),
  syntax: $("#syntax"),
  syntaxWrap: $("#syntaxWrap"),
  opt: $("#opt"),
  optWrap: $("#optWrap"),
  maxSteps: $("#maxSteps"),
  examples: $("#examples"),
  btnCompile: $("#btnCompile"),
  btnRun: $("#btnRun"),
  tools: $("#tools"),
  asmView: $("#asmView"),
  asmTitle: $("#asmTitle"),
  showDir: $("#showDir"),
  dirWrap: $("#dirWrap"),
  tabDisasm: $("#tabDisasm"),
  tabS: $("#tabS"),
  tabHex: $("#tabHex"),
  codeTitle: $("#codeTitle"),
  brandLang: $("#brandLang"),
  brandTarget: $("#brandTarget"),
  regs: $("#regs"),
  memView: $("#memView"),
  timeline: $("#timeline"),
  stepInfo: $("#stepInfo"),
  bPlay: $("#bPlay"),
  playMode: $("#playMode"),
  speed: $("#speed"),
  stdout: $("#stdout"),
  diag: $("#diag"),
  inputBar: $("#inputBar"),
  inputText: $("#inputText"),
  btnSend: $("#btnSend"),
  btnEof: $("#btnEof"),
};

const EMPTY_MEM =
  '<div class="empty">Clique em “Executar passo a passo” para acompanhar registradores, pilha, heap e dados globais.</div>';
const EMPTY_ASM =
  '<div class="empty">Compile um programa para ver o resultado.</div>';

// ======================================================================
//  Editor
// ======================================================================
function updateGutter() {
  const n = el.src.value.split("\n").length;
  let s = "";
  for (let i = 1; i <= n; i++) s += i + "\n";
  el.gutter.textContent = s;
  el.gutter.scrollTop = el.src.scrollTop;
}

el.src.addEventListener("input", () => {
  updateGutter();
  store.set("asmviz.code." + S.lang, el.src.value);
});
el.src.addEventListener("scroll", () => {
  el.gutter.scrollTop = el.src.scrollTop;
});
el.src.addEventListener("keydown", (e) => {
  if (e.key === "Tab") {
    e.preventDefault();
    const { selectionStart: a, selectionEnd: b, value } = el.src;
    el.src.value = value.slice(0, a) + "    " + value.slice(b);
    el.src.selectionStart = el.src.selectionEnd = a + 4;
    el.src.dispatchEvent(new Event("input"));
  }
});

function setEditing(on) {
  el.editor.classList.toggle("hidden", !on);
  el.codeView.classList.toggle("hidden", on);
  el.btnEdit.classList.toggle("hidden", on);
  if (on) {
    stopPlay();
    el.src.focus();
  }
}
el.btnEdit.addEventListener("click", () => setEditing(true));

// ======================================================================
//  Linguagem
// ======================================================================
function applyLangUI() {
  const java = isJava();
  el.archWrap.classList.toggle("hidden", java);
  el.optWrap.classList.toggle("hidden", java);
  el.syntaxWrap.classList.toggle("hidden", java || el.arch.value !== "x86_64");
  el.tabS.classList.toggle("hidden", java);
  el.tabHex.classList.toggle("hidden", java);
  el.tabDisasm.textContent = java ? "javap -c" : "Binário";
  if (!java) {
    el.tabS.textContent = S_TAB[S.lang];
    el.tabS.title = `Saída do compilador (${S_TAB[S.lang]})`;
  }
  const ln = LANG_NAME[S.lang];
  $("#bPrevLine").textContent = "⏪ " + ln;
  $("#bPrevLine").title = `Linha ${ln} anterior (↑)`;
  $("#bNextLine").textContent = ln + " ⏩";
  $("#bNextLine").title = `Próxima linha ${ln} (↓)`;
  el.playMode.options[0].textContent = "linha " + ln;
  el.tabDisasm.title = java
    ? "Bytecode da JVM (javap -c)"
    : "Desmontagem do binário com endereços reais (llvm-objdump)";
  el.asmTitle.textContent = java ? "Bytecode (JVM)" : "Assembly";
  el.codeTitle.textContent = "Código " + LANG_NAME[S.lang];
  el.brandLang.textContent = LANG_NAME[S.lang];
  el.brandTarget.textContent = java ? "Bytecode" : "Assembly";
  if (java) {
    S.asmTab = "disasm";
    S.memTab = "map";
  }
  el.examples.innerHTML =
    '<option value="">— escolher —</option>' +
    Object.keys(EXAMPLES[S.lang])
      .map((k) => `<option>${esc(k)}</option>`)
      .join("");
  syncTabs();
}

function setLang(lang, loadCode = true) {
  cancelSession();
  S.lang = lang;
  el.lang.value = lang;
  store.set("asmviz.lang", lang);
  if (loadCode) {
    el.src.value = store.get(
      "asmviz.code." + lang,
      EXAMPLES[lang][DEFAULT_EXAMPLE[lang]],
    );
    updateGutter();
  }
  S.result = null;
  clearTrace();
  el.asmView.innerHTML = EMPTY_ASM;
  el.stdout.textContent = "";
  applyLangUI();
  setEditing(true);
}

el.lang.addEventListener("change", () => setLang(el.lang.value));

// ======================================================================
//  Comunicação com o servidor
// ======================================================================
const HTTP_HINTS = {
  413: "requisição grande demais para o proxy",
  502: "o proxy não alcançou o servidor (contêiner parado?)",
  503: "servidor indisponível",
  504: "o proxy desistiu de esperar a resposta (tempo esgotado)",
};

/// Erro de comunicação com uma mensagem legível para o usuário.
class ServerError extends Error {}

/// POST com JSON. Respostas JSON do servidor são devolvidas mesmo com status de
/// erro (elas trazem `diagnostics`); qualquer outra coisa (página de erro de um
/// proxy, falha de rede) vira ServerError com o motivo.
async function postJson(url, body) {
  let resp;
  try {
    resp = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
  } catch {
    throw new ServerError(
      "sem resposta (servidor offline ou rede indisponível)",
    );
  }
  const text = await resp.text();
  if ((resp.headers.get("Content-Type") || "").includes("application/json")) {
    try {
      return JSON.parse(text);
    } catch {
      /* cai no erro abaixo */
    }
  }
  const status = `HTTP ${resp.status}${resp.statusText ? " " + resp.statusText : ""}`;
  const hint =
    HTTP_HINTS[resp.status] ||
    (resp.ok ? "resposta não é JSON" : "erro do servidor ou do proxy");
  throw new ServerError(`${status} — ${hint}`);
}
async function loadTools() {
  try {
    const t = await (await fetch("/api/tools")).json();
    const items = [
      ["clang", t.clang],
      ["clang++", t["clang++"]],
      ["lld", t.lld],
      ["objdump", t.objdump],
      ["gdb", t.gdb],
      ["qemu-aarch64", t.qemu],
      ["sysroot ARM64", t.aarch64Sysroot],
      ["gcc ARM64", t.aarch64Gcc],
      ["rustc", t.rustc],
      ["javac", t.javac],
      ["jdb", t.jdb],
    ];
    el.tools.innerHTML = items
      .map(
        ([n, ok]) =>
          `<span class="pill ${ok ? "ok" : "bad"}" title="${esc(ok || "não encontrado")}">${ok ? "✓" : "✗"} ${n}</span>`,
      )
      .join("");
  } catch {
    el.tools.innerHTML =
      '<span class="pill bad">servidor offline — rode: cargo run --release</span>';
  }
}

async function build(trace) {
  if (S.busy) return;
  cancelSession();
  S.busy = true;
  stopPlay();
  el.btnCompile.disabled = el.btnRun.disabled = true;
  el.diag.innerHTML = `<span class="spinner"></span> ${trace ? "Compilando e executando (pode levar alguns segundos)..." : "Compilando..."}`;
  const body = {
    lang: S.lang,
    code: el.src.value,
    arch: el.arch.value,
    syntax: el.syntax.value,
    opt: el.opt.value,
    stdin: el.stdin.value,
    maxSteps: +el.maxSteps.value || 3000,
    trace,
  };
  try {
    const r = await postJson("/api/build", body);
    if (r.lang && r.lang !== S.lang) {
      // linguagem mudou durante a requisição
      if (r.session != null) {
        S.session = r.session;
        cancelSession();
      }
      return;
    }
    onResult(r, trace);
  } catch (err) {
    el.diag.innerHTML = `<span class="err">Falha ao contatar o servidor: ${esc(err.message)}</span>`;
    if (!(err instanceof ServerError)) console.error(err);
  } finally {
    S.busy = false;
    el.btnCompile.disabled = el.btnRun.disabled = false;
  }
}

function renderDiag(r) {
  let html = "";
  for (const c of r.commands || [])
    html += `<span class="cmd">$ ${esc(c)}</span>\n`;
  const d = (r.diagnostics || "").trim();
  if (d) {
    html +=
      d
        .split("\n")
        .map((l) => {
          const cls = /error|erro/i.test(l)
            ? "err"
            : /warning/i.test(l)
              ? "warn"
              : "";
          return cls ? `<span class="${cls}">${esc(l)}</span>` : esc(l);
        })
        .join("\n") + "\n";
  }
  const t = r.trace;
  if (t) {
    const st =
      {
        returned: "main retornou",
        exited: "programa terminou",
        limit: "limite de passos atingido",
        timeout: "tempo limite atingido",
        error: "erro",
        signal: "sinal recebido",
        exception: "exceção não tratada",
        input: "pausado esperando entrada do usuário",
      }[t.status] ||
      t.status ||
      "";
    html +=
      `\n<span class="${t.error ? "err" : ""}">Trace: ${t.steps ? t.steps.length : 0} passos — ${esc(st)}` +
      (t.exitCode != null ? ` (código de saída ${t.exitCode})` : "") +
      "</span>\n";
    if (t.error) html += `<span class="err">${esc(t.error)}</span>\n`;
    if ((t.error || !t.steps || !t.steps.length) && t.log)
      html += `<span class="cmd">${esc(t.log)}</span>\n`;
  } else if (r.ok && !r.linkError) {
    html += "\nCompilado com sucesso.";
  }
  el.diag.innerHTML = html || "OK";
}

function onResult(r, trace) {
  S.result = r;
  S.steps = (r.trace && r.trace.steps) || [];
  S.meta = r.trace && r.trace.meta;
  S.idx = 0;
  renderDiag(r);
  el.stdout.textContent = "";
  if (!r.ok) {
    el.asmView.innerHTML =
      '<div class="empty">Erro de compilação — veja o painel “Compilador / depurador”.</div>';
    clearTrace();
    return;
  }
  if (!r.disasm && S.asmTab === "disasm") S.asmTab = "s";
  syncTabs();
  setEditing(false);
  el.timeline.max = Math.max(0, S.steps.length - 1);
  el.timeline.value = 0;
  el.timeline.disabled = !S.steps.length;
  if (!S.steps.length) clearTrace();
  render();
  if (
    trace &&
    r.trace &&
    !S.steps.length &&
    !r.trace.error &&
    r.trace.status !== "input"
  ) {
    el.diag.innerHTML += '\n<span class="warn">Nenhum passo registrado.</span>';
  }
  if (r.trace && r.trace.status === "input") pauseForInput(r.session);
}

// ======================================================================
//  Entrada interativa: o programa pausou lendo o stdin
// ======================================================================
function pauseForInput(session) {
  S.session = session;
  if (S.steps.length) go(S.steps.length - 1); // a chamada que pediu a entrada
  if (!S.steps.length) el.stdout.textContent = S.result.trace.stdout || "";
  el.inputBar.classList.remove("hidden");
  el.inputText.disabled = el.btnSend.disabled = el.btnEof.disabled = false;
  el.inputText.value = "";
  el.inputText.focus();
}

function hideInput() {
  el.inputBar.classList.add("hidden");
}

/// Encerra no servidor uma execução pausada que não será mais usada.
function cancelSession() {
  if (S.session == null) return;
  const session = S.session;
  S.session = null;
  hideInput();
  postJson("/api/input", { session, cancel: true }).catch(() => {});
}

async function sendInput(payload) {
  if (S.session == null || S.busy) return;
  const session = S.session;
  const lang = S.lang;
  S.busy = true;
  el.inputText.disabled = el.btnSend.disabled = el.btnEof.disabled = true;
  el.btnCompile.disabled = el.btnRun.disabled = true;
  const from = S.steps.length;
  el.diag.innerHTML +=
    '\n<span class="spinner"></span> Continuando a execução...';
  try {
    const r = await postJson("/api/input", { session, ...payload });
    if (S.session !== session || S.lang !== lang) return; // outra execução começou
    S.session = null;
    hideInput();
    if (!r.trace) {
      renderDiag(S.result);
      el.diag.innerHTML += `\n<span class="err">${esc(r.diagnostics || "erro")}</span>`;
      return;
    }
    const t = r.trace;
    if (t.offset === from) S.steps.push(...(t.steps || []));
    Object.assign(S.result.trace, t, { steps: S.steps });
    renderDiag(S.result);
    el.timeline.max = Math.max(0, S.steps.length - 1);
    el.timeline.disabled = !S.steps.length;
    if (S.steps.length > from)
      go(from); // primeiro passo após a entrada
    else render();
    if (t.status === "input") pauseForInput(r.session);
  } catch (err) {
    S.session = null;
    hideInput();
    el.diag.innerHTML += `\n<span class="err">Falha ao contatar o servidor: ${esc(err.message)}</span>`;
    if (!(err instanceof ServerError)) console.error(err);
  } finally {
    S.busy = false;
    el.btnCompile.disabled = el.btnRun.disabled = false;
  }
}

el.inputBar.addEventListener("submit", (e) => {
  e.preventDefault();
  sendInput({ text: el.inputText.value + "\n" });
});
el.btnEof.onclick = () => sendInput({ eof: true });
// fechar a página libera a execução pausada no servidor
window.addEventListener("pagehide", () => {
  if (S.session != null)
    navigator.sendBeacon(
      "/api/input",
      JSON.stringify({ session: S.session, cancel: true }),
    );
});
el.inputText.addEventListener("keydown", (e) => {
  if (e.ctrlKey && (e.key === "d" || e.key === "D")) {
    e.preventDefault();
    sendInput({ eof: true });
  }
});

function clearTrace() {
  S.steps = [];
  el.regs.innerHTML = "";
  el.regs.className = "regs";
  el.memView.innerHTML = EMPTY_MEM;
  el.stepInfo.textContent = "—";
  el.timeline.disabled = true;
}

// ======================================================================
//  Renderização geral
// ======================================================================
const cur = () => S.steps[S.idx] || null;
const prevStep = () => (S.idx > 0 ? S.steps[S.idx - 1] : null);

function render() {
  renderCode();
  renderAsm();
  renderStep();
}

function asmLinesFor() {
  const r = S.result;
  if (!r) return [];
  return S.asmTab === "disasm" ? r.disasm || [] : r.asm || [];
}

function clinesWithAsm() {
  const set = new Set();
  for (const a of (S.result && (S.result.disasm || S.result.asm)) || [])
    if (a.cline) set.add(a.cline);
  return set;
}

// ---------------- código fonte ----------------
function renderCode() {
  const lines = el.src.value.split("\n");
  const mapped = clinesWithAsm();
  const st = cur();
  const curLine = st ? st.line : null;
  // linhas das chamadas ativas (frames mais antigos)
  const callers = new Set(st ? st.frames.slice(1).map((f) => f.line) : []);
  el.codeView.innerHTML = lines
    .map((l, i) => {
      const n = i + 1;
      const color = mapped.has(n) ? lineColor(n) : "transparent";
      const cls = ["cl"];
      if (n === curLine) cls.push("cur");
      else if (callers.has(n) || n === S.hoverLine) cls.push("hl");
      return (
        `<div class="${cls.join(" ")}" data-line="${n}" style="border-left-color:${color}">` +
        `<span class="n">${n}</span><span class="c">${esc(l) || " "}</span></div>`
      );
    })
    .join("");
  if (curLine)
    scrollIntoViewIfNeeded(el.codeView, el.codeView.querySelector(".cl.cur"));
}

el.codeView.addEventListener("mouseover", (e) => {
  const d = e.target.closest(".cl");
  setHover(d ? +d.dataset.line : null);
});
el.codeView.addEventListener("mouseleave", () => setHover(null));

function setHover(line) {
  if (line === S.hoverLine) return;
  S.hoverLine = line;
  const st = cur();
  for (const d of el.codeView.querySelectorAll(".cl")) {
    const n = +d.dataset.line;
    d.classList.toggle("hl", n === line && !(st && st.line === n));
  }
  for (const d of el.asmView.querySelectorAll(".al")) {
    d.classList.toggle("hl", line != null && +d.dataset.cline === line);
  }
}

// ---------------- assembly / bytecode ----------------
const REG_RE = String.raw`%[a-z0-9]+|\b(?:[re]?[abcd]x|[abcd][lh]|[re]?(?:si|di|bp|sp|ip)l?|r(?:[89]|1[0-5])[dwb]?|[xwqdshbv](?:[12]?\d|3[01])|xmm\d+|ymm\d+|w?sp|[xw]zr|lr|fp|nzcv)\b`;
const TOK_RE = new RegExp(
  String.raw`(\s(?:#(?!\d)|//|;).*$)|(<[^>]+>)|(${REG_RE})|([$#]-?(?:0x[0-9a-f]+|\d+)|\b0x[0-9a-f]+\b|\b\d+\b)`,
  "gi",
);
const JTOK_RE = /(\/\/.*$)|(#\d+(?:,\s*\d+)?)|(\b-?\d+\b)/g;

function hlAsm(text, java) {
  const m = /^(\s*)(\S+)(.*)$/.exec(text);
  if (!m) return esc(text);
  const [, lead, mnem, rest] = m;
  let out = esc(lead) + `<span class="tk-m">${esc(mnem)}</span>`;
  const re = java ? JTOK_RE : TOK_RE;
  let last = 0;
  re.lastIndex = 0;
  let t;
  while ((t = re.exec(rest))) {
    out += esc(rest.slice(last, t.index));
    const cls = java
      ? t[1]
        ? "tk-c"
        : t[2]
          ? "tk-s"
          : "tk-i"
      : t[1]
        ? "tk-c"
        : t[2]
          ? "tk-s"
          : t[3]
            ? "tk-r"
            : "tk-i";
    out += `<span class="${cls}">${esc(t[0])}</span>`;
    last = t.index + t[0].length;
    if (t[0].length === 0) re.lastIndex++;
  }
  return out + esc(rest.slice(last));
}

function isPcLine(a, st) {
  if (!st || a.kind !== "insn" || a.addr == null) return false;
  return resultIsJava()
    ? a.m === st.func && a.addr === st.bci
    : a.addr === st.pc;
}

function renderAsm() {
  const r = S.result;
  el.syntaxWrap.classList.toggle(
    "hidden",
    isJava() || el.arch.value !== "x86_64",
  );
  el.dirWrap.classList.toggle("hidden", S.asmTab !== "s");
  if (!r || !r.ok) return;
  const java = resultIsJava();
  const lines = asmLinesFor();
  if (!lines.length) {
    el.asmView.innerHTML = `<div class="empty">${
      S.asmTab === "disasm"
        ? `Desmontagem indisponível (falha na ligação). Use a aba “${S_TAB[S.lang] || "clang -S"}”.`
        : "Sem assembly."
    }</div>`;
    return;
  }
  const st = cur();
  const showDir = el.showDir.checked;
  let html = "";
  for (const a of lines) {
    if (S.asmTab === "s" && !showDir) {
      if (a.kind === "dir") continue;
      if (
        a.kind === "label" &&
        /^\.L(tmp|func_end|func_begin)/.test(a.t.trim())
      )
        continue;
    }
    const cls = ["al", a.kind];
    // em Java, a mesma linha pode existir em métodos diferentes: limita ao método atual
    const sameLine =
      st && a.cline && a.cline === st.line && (!java || a.m === st.func);
    if (sameLine) cls.push("cur");
    if (isPcLine(a, st)) cls.push("pc");
    if (S.hoverLine && a.cline === S.hoverLine) cls.push("hl");
    const color = a.cline ? lineColor(a.cline) : "transparent";
    const addr =
      a.addr != null && a.kind === "insn"
        ? java
          ? String(a.addr)
          : a.addr.toString(16)
        : "";
    const code =
      a.kind === "insn"
        ? hlAsm(a.t.replace(/^\s+/, S.asmTab === "s" ? "    " : ""), java)
        : esc(a.t);
    html +=
      `<div class="${cls.join(" ")}" data-cline="${a.cline || ""}" style="border-left-color:${color}">` +
      (S.asmTab === "disasm" ? `<span class="addr">${addr}</span>` : "") +
      `<span class="code">${code}</span></div>`;
  }
  el.asmView.innerHTML = html;
  const target =
    el.asmView.querySelector(".al.pc") || el.asmView.querySelector(".al.cur");
  if (target) scrollIntoViewIfNeeded(el.asmView, target);
}

el.asmView.addEventListener("mouseover", (e) => {
  const d = e.target.closest(".al");
  const n = d && d.dataset.cline ? +d.dataset.cline : null;
  setHover(n);
  if (n) {
    const c = el.codeView.querySelector(`.cl[data-line="${n}"]`);
    if (c) scrollIntoViewIfNeeded(el.codeView, c);
  }
});
el.asmView.addEventListener("mouseleave", () => setHover(null));

function scrollIntoViewIfNeeded(container, node) {
  if (!node) return;
  const c = container.getBoundingClientRect();
  const n = node.getBoundingClientRect();
  if (n.top < c.top + 20 || n.bottom > c.bottom - 20) {
    container.scrollTop += n.top - c.top - c.height / 3;
  }
}

// ======================================================================
//  Passo atual
// ======================================================================
function insnAt(st) {
  const d = S.result && S.result.disasm;
  if (!d) return "";
  const a = d.find((x) => isPcLine(x, st));
  return a ? a.t : "";
}

function renderStep() {
  const st = cur();
  if (!st) return;
  const prev = prevStep();
  const n = S.steps.length;
  const java = resultIsJava();
  el.timeline.value = S.idx;
  const where = java ? `bci=<b>${st.bci}</b>` : `pc=<b>${hx(st.pc)}</b>`;
  el.stepInfo.innerHTML =
    `passo <b>${S.idx + 1}</b>/${n} · linha <b>${st.line ?? "?"}</b> · <b>${esc(st.func || "?")}()</b> · ${where}<br>` +
    `<span style="color:var(--text)">${esc(insnAt(st))}</span>`;
  const out = (S.result.trace && S.result.trace.stdout) || "";
  el.stdout.textContent = out.slice(
    0,
    st.outLen != null ? st.outLen : out.length,
  );
  el.stdout.scrollTop = el.stdout.scrollHeight;
  if (java) {
    renderJvmInfo(st);
    renderJavaMap(st, prev);
    return;
  }
  renderRegs(st, prev);
  if (S.memTab === "map") renderMap(st, prev);
  else renderHex(st, prev);
}

function renderRegs(st, prev) {
  const m = S.meta;
  const special = new Set([m.sp, m.fp, m.pc]);
  el.regs.className = "regs";
  el.regs.innerHTML = m.regs
    .filter((r) => r in st.regs)
    .map((r) => {
      const v = st.regs[r];
      const chg = prev && prev.regs[r] !== v && r !== m.pc;
      const shown = /^0x/.test(v)
        ? "0x" + v.slice(2).replace(/^0+(?=.)/, "")
        : v;
      const label = r === "x29" ? "x29/fp" : r === "x30" ? "x30/lr" : r;
      return `<div class="reg${chg ? " chg" : ""}${special.has(r) ? " special" : ""}" title="${esc(v)}"><b>${label}</b>${esc(shown)}</div>`;
    })
    .join("");
}

// ---------- helpers de memória (C/C++) ----------
function stackRange(st) {
  const lo = st.stack.lo;
  return [lo, lo + st.stack.hex.length / 2];
}

function sectionOf(addr) {
  for (const s of S.result.sections || [])
    if (addr >= s.addr && addr < s.addr + s.size) return s.name;
  return null;
}

function allVars(st) {
  const list = [];
  st.frames.forEach((f, fi) =>
    f.vars.forEach((v) => list.push({ ...v, func: f.func, fi })),
  );
  (st.globals || []).forEach((v) => list.push({ ...v, global: true }));
  return list;
}

function varKey(v, st) {
  if (v.global) return "g:" + v.name;
  return `f${st.frames.length - v.fi}:${v.func}:${v.name}`;
}

function describePtr(p, st) {
  if (!p) return "NULL";
  for (const v of allVars(st)) {
    if (v.addr != null && v.size && p >= v.addr && p < v.addr + v.size) {
      const off = p - v.addr;
      return `&${v.name}${off ? "+" + off : ""}`;
    }
  }
  const [lo, hi] = stackRange(st);
  if (p >= lo && p < hi) return "pilha";
  const sec = sectionOf(p);
  if (sec) return sec;
  const fn = (S.result.functions || []).find((f) => p >= f.start && p < f.end);
  if (fn) return fn.name + "()";
  return "heap";
}

function bytesOf(hex) {
  const b = [];
  for (let i = 0; i < hex.length; i += 2) b.push(hex.substr(i, 2));
  return b;
}

function asciiOf(bytes) {
  return bytes
    .map((b) => {
      const c = parseInt(b, 16);
      return c >= 32 && c < 127 ? String.fromCharCode(c) : "·";
    })
    .join("");
}

function varRow(v, st, prevMap, colorIdx) {
  const key = varKey(v, st);
  const chg = prevMap && prevMap.has(key) && prevMap.get(key) !== v.value;
  const val = v.error
    ? `<span style="color:var(--err)">${esc(v.error)}</span>`
    : esc(v.value);
  const ptr =
    v.ptr != null
      ? ` <span class="ptr">→ ${esc(describePtr(v.ptr, st))}</span>`
      : "";
  const sw =
    colorIdx != null
      ? `<span class="sw" style="background:${lineColor(colorIdx + 3, 0.9)}"></span>`
      : "";
  return (
    `<tr class="${chg ? "chg" : ""}"><td class="a">${hx(v.addr)}</td>` +
    `<td class="nm">${sw}${esc(v.name)} <i>${esc(v.type || "")}${v.arg ? " · arg" : ""}</i></td>` +
    `<td class="v">${val}${ptr}</td></tr>`
  );
}

function prevValueMap(prev) {
  if (!prev) return null;
  const m = new Map();
  for (const v of allVars(prev)) m.set(varKey(v, prev), v.value);
  return m;
}

// ---------- aba "Mapa" (C/C++) ----------
function renderMap(st, prev) {
  const prevMap = prevValueMap(prev);
  const [lo, hi] = stackRange(st);
  const inStack = (a) => a != null && a >= lo - 4096 && a < hi + 4096;
  const statics = [];
  const colors = stackColors(st);
  let html = "";

  // Pilha: frame mais externo (main) no topo, endereços altos acima.
  html += `<div class="region stack"><div class="rh">Pilha (stack) <small>endereços altos ↑ · cresce para baixo ↓</small></div>`;
  for (let fi = st.frames.length - 1; fi >= 0; fi--) {
    const f = st.frames[fi];
    const locals = [];
    for (const v of f.vars) {
      if (v.addr != null && !inStack(v.addr))
        statics.push({ ...v, func: f.func, fi });
      else locals.push({ ...v, func: f.func, fi });
    }
    locals.sort((a, b) => (b.addr || 0) - (a.addr || 0));
    const size = f.top && f.sp ? f.top - f.sp : null;
    html +=
      `<div class="frame${fi === 0 ? " curf" : ""}"><div class="fh">` +
      `<span class="fn">${esc(f.func)}()</span>` +
      `<span class="meta">linha ${f.line ?? "?"}</span>` +
      `<span class="meta">${S.meta.fp}=${hx(f.fp)}</span>` +
      `<span class="meta">${S.meta.sp}=${hx(f.sp)}</span>` +
      (size != null ? `<span class="meta">${size} bytes</span>` : "") +
      `</div>`;
    // Durante o prólogo/epílogo as variáveis (endereçadas a partir de fp ou sp)
    // caem fora do frame desta chamada: os valores mostrados não são delas.
    const floor = f.sp != null ? f.sp - (S.meta.redZone || 0) : null;
    const unbuilt =
      fi === 0 &&
      floor != null &&
      f.top != null &&
      locals.some(
        (v) =>
          v.addr != null && (v.addr < floor || v.addr + (v.size || 1) > f.top),
      );
    if (unbuilt) {
      html += `<div class="note" style="color:var(--warn)">Prólogo/epílogo em execução: o frame desta chamada ainda não está montado (ou já foi desfeito). Os valores abaixo não são das variáveis desta chamada.</div>`;
    }
    html += locals.length
      ? `<table class="vars"${unbuilt ? ' style="opacity:.45"' : ""}>${locals.map((v) => varRow(v, st, prevMap, colors.get(`${fi}:${v.name}`))).join("")}</table>`
      : '<div class="note">sem variáveis locais</div>';
    html += "</div>";
  }
  html += `<div class="note">◀ ${S.meta.sp} = ${hx(st.frames[0] ? st.frames[0].sp : null)} (topo da pilha)</div></div>`;
  html += '<div class="gap">⋮ espaço livre ⋮</div>';

  // Heap e outros dados apontados por ponteiros
  const heap = [],
    ro = [];
  for (const p of st.pointees) {
    const sec = sectionOf(p.addr);
    if (sec === ".rodata" || sec === ".text") ro.push({ ...p, sec });
    else if (!sec) heap.push(p);
  }
  html += `<div class="region heap"><div class="rh">Heap <small>memória dinâmica (malloc / new) apontada por variáveis</small></div>`;
  if (heap.length) {
    for (const p of heap.sort((a, b) => b.addr - a.addr))
      html += pointeeBlock(p, prev);
  } else {
    html += '<div class="note">nenhum ponteiro para o heap no momento</div>';
  }
  html += "</div>";

  // Dados globais / estáticos
  const globals = (st.globals || []).map((v) => ({ ...v, global: true }));
  html += `<div class="region data"><div class="rh">Dados globais e estáticos <small>.data / .bss / .rodata</small></div>`;
  if (globals.length || statics.length) {
    const rows = [...globals, ...statics].sort(
      (a, b) => (b.addr || 0) - (a.addr || 0),
    );
    html +=
      '<table class="vars">' +
      rows
        .map((v) => {
          const sec = v.addr != null ? sectionOf(v.addr) : null;
          const label = v.global ? v.name : `${v.func}::${v.name}`;
          return varRow(
            {
              ...v,
              name: label,
              type: `${v.type || ""}${sec ? " · " + sec : ""}`,
            },
            st,
            prevMap,
            null,
          );
        })
        .join("") +
      "</table>";
  } else {
    html += '<div class="note">sem variáveis globais</div>';
  }
  if (ro.length) {
    html += '<div class="note">Constantes apontadas (.rodata):</div>';
    for (const p of ro) html += pointeeBlock(p, prev, true);
  }
  html += "</div>";

  // Código
  html += `<div class="region text"><div class="rh">Código (.text) <small>funções do programa</small></div><table class="vars">`;
  for (const f of (S.result.functions || [])
    .slice()
    .sort((a, b) => b.start - a.start)) {
    const here = st.pc >= f.start && st.pc < f.end;
    html +=
      `<tr><td class="a">${hx(f.start)}</td><td class="nm">${esc(f.name)} <i>${f.end - f.start} bytes</i></td>` +
      `<td class="v">${here ? `<span class="ptr">◀ pc = ${hx(st.pc)} (+${st.pc - f.start})</span>` : ""}</td></tr>`;
  }
  html += "</table></div>";

  const scroll = el.memView.scrollTop;
  el.memView.innerHTML = html;
  el.memView.scrollTop = scroll;
}

function pointeeBlock(p, prev, asString = false) {
  const bytes = bytesOf(p.hex);
  const old = prev && prev.pointees.find((q) => q.addr === p.addr);
  const oldBytes = old ? bytesOf(old.hex) : null;
  if (asString) {
    const end = bytes.indexOf("00");
    const s = asciiOf(end >= 0 ? bytes.slice(0, end) : bytes);
    return `<div class="bytes">${hx(p.addr)} ← <b>${esc(p.from)}</b>: "${esc(s)}"</div>`;
  }
  let rows = "";
  for (let i = 0; i < bytes.length; i += 16) {
    const chunk = bytes.slice(i, i + 16);
    const cells = chunk
      .map((b, j) => {
        const chg = oldBytes && oldBytes[i + j] !== b;
        return `<span class="b${chg ? " chg" : ""}">${b}</span>`;
      })
      .join(" ");
    rows += `<tr><td class="a">${hx(p.addr + i)}</td><td>${cells}</td><td class="q">${esc(asciiOf(chunk))}</td></tr>`;
  }
  return (
    `<div class="note">${hx(p.addr)} ← apontado por <b>${esc(p.from)}</b> <i>(${esc(p.type)})</i> · primeiros ${bytes.length} bytes</div>` +
    `<table class="hex">${rows}</table>`
  );
}

// cores das variáveis da pilha (mesma cor no mapa e no dump de bytes)
function stackColors(st) {
  const m = new Map();
  let i = 0;
  st.frames.forEach((f, fi) =>
    f.vars.forEach((v) => {
      m.set(`${fi}:${v.name}`, i++);
    }),
  );
  return m;
}

// ---------- aba "Pilha (bytes)" (C/C++) ----------
function renderHex(st, prev) {
  const [lo, hi] = stackRange(st);
  const bytes = bytesOf(st.stack.hex);
  const colors = stackColors(st);
  const owner = new Map();
  const starts = new Map();
  st.frames.forEach((f, fi) =>
    f.vars.forEach((v) => {
      if (v.addr == null || !v.size || v.addr < lo || v.addr >= hi) return;
      const ci = colors.get(`${fi}:${v.name}`);
      for (let a = v.addr; a < v.addr + v.size; a++)
        owner.set(a, { name: v.name, ci });
      const rowAddr = v.addr - ((v.addr - lo) % 8);
      if (!starts.has(rowAddr)) starts.set(rowAddr, []);
      starts
        .get(rowAddr)
        .push(
          `${v.name}${v.addr !== rowAddr ? "@+" + (v.addr - rowAddr) : ""}`,
        );
    }),
  );

  const prevBytes = new Map();
  if (prev) {
    const pb = bytesOf(prev.stack.hex);
    pb.forEach((b, i) => prevBytes.set(prev.stack.lo + i, b));
  }

  const marks = new Map();
  const mark = (a, txt) => {
    if (a == null) return;
    const r = a - ((a - lo) % 8);
    if (!marks.has(r)) marks.set(r, []);
    marks.get(r).push(txt);
  };
  const sp = st.frames[0] ? st.frames[0].sp : null;
  mark(sp, `◀ ${S.meta.sp}`);
  st.frames.forEach((f, fi) => {
    if (f.fp != null && f.fp >= lo && f.fp < hi)
      mark(f.fp, `◀ ${S.meta.fp} de ${f.func}()${fi ? "" : " (atual)"}`);
    if (S.meta.pc === "rip" && f.top)
      mark(f.top - 8, `endereço de retorno de ${f.func}()`);
  });

  let rows = "";
  for (let r = lo + Math.floor((bytes.length - 1) / 8) * 8; r >= lo; r -= 8) {
    const i0 = r - lo;
    const chunk = bytes.slice(i0, i0 + 8);
    const cells = chunk
      .map((b, j) => {
        const a = r + j;
        const o = owner.get(a);
        const chg = prev && prevBytes.has(a) && prevBytes.get(a) !== b;
        const style = o
          ? ` style="background:${lineColor(o.ci + 3, 0.28)}"`
          : "";
        return `<span class="b${chg ? " chg" : ""}"${style} title="${hx(a)}${o ? " · " + esc(o.name) : ""}">${b}</span>`;
      })
      .join(" ");
    const qword =
      chunk.length === 8
        ? "0x" +
          chunk
            .slice()
            .reverse()
            .join("")
            .replace(/^0+(?=.)/, "")
        : "";
    const labels = [
      ...(starts.get(r) || []).map(
        (n) => `<span style="color:var(--text)">${esc(n)}</span>`,
      ),
      ...(marks.get(r) || []).map(esc),
    ];
    const red = sp != null && r + 8 <= sp;
    rows +=
      `<tr class="${r === sp ? "sp" : ""}${red ? " red" : ""}"><td class="a">${hx(r)}</td><td>${cells}</td>` +
      `<td class="q">${qword}</td><td class="lab">${labels.join(" · ")}</td></tr>`;
  }
  const note =
    S.meta.redZone && sp != null
      ? `<div class="note">Linhas esmaecidas abaixo de ${S.meta.sp}: “red zone” de ${S.meta.redZone} bytes (x86-64 System V) que funções folha podem usar sem mover ${S.meta.sp}.</div>`
      : "";
  const scroll = el.memView.scrollTop;
  el.memView.innerHTML =
    `<div class="note">Bytes da pilha de ${hx(lo)} a ${hx(hi)} (endereços altos no topo). Cada linha = 8 bytes; coluna à direita = valor little-endian de 64 bits. Bytes alterados neste passo ficam destacados.</div>${note}` +
    `<table class="hex">${rows}</table>`;
  el.memView.scrollTop = scroll;
}

// ---------- Java: frames da JVM, heap e campos static ----------
function renderJvmInfo(st) {
  el.regs.className = "jinfo";
  el.regs.innerHTML =
    `<span><b>thread</b>main</span>` +
    `<span><b>método</b>${esc(st.func)}</span>` +
    `<span><b>bci (pc da JVM)</b>${st.bci}</span>` +
    `<span><b>frames</b>${st.frames.length}</span>` +
    `<span style="color:var(--muted)">A pilha de operandos não é exposta pelo JDWP/jdb.</span>`;
}

const objId = (v) => {
  const m = /\(id=(\d+)\)/.exec(v || "");
  return m ? +m[1] : null;
};

function renderJavaMap(st, prev) {
  const prevVals = new Map();
  if (prev) {
    prev.frames.forEach((f, fi) =>
      f.vars.forEach((v) =>
        prevVals.set(`${prev.frames.length - fi}:${f.func}:${v.name}`, v.value),
      ),
    );
    (prev.heap || []).forEach((h) => prevVals.set("h:" + h.id, h.value));
    (prev.statics || []).forEach((s) =>
      prevVals.set(`s:${s.class}.${s.name}`, s.value),
    );
  }
  const changed = (k, v) => prev && prevVals.has(k) && prevVals.get(k) !== v;
  const valueCell = (value) => {
    const id = objId(value);
    return id != null
      ? `<span class="obj">→ objeto #${id}</span> <i style="color:var(--muted)">${esc(value.replace(/\s*\(id=\d+\)/, "").replace("instance of ", ""))}</i>`
      : esc(value);
  };

  let html = `<div class="region jvm"><div class="rh">Pilha da JVM <small>thread main · um frame por chamada · variáveis locais por slot</small></div>`;
  for (let fi = st.frames.length - 1; fi >= 0; fi--) {
    const f = st.frames[fi];
    const vars = f.vars.slice().sort((a, b) => (a.slot ?? 99) - (b.slot ?? 99));
    html +=
      `<div class="frame${fi === 0 ? " curf" : ""}"><div class="fh">` +
      `<span class="fn">${esc(f.func)}()</span>` +
      `<span class="meta">linha ${f.line ?? "?"}</span>` +
      (f.bci != null ? `<span class="meta">bci ${f.bci}</span>` : "") +
      `</div>`;
    if (f.note) html += `<div class="note">${esc(f.note)}</div>`;
    html += vars.length
      ? '<table class="vars">' +
        vars
          .map((v) => {
            const chg = changed(
              `${st.frames.length - fi}:${f.func}:${v.name}`,
              v.value,
            );
            return (
              `<tr class="${chg ? "chg" : ""}"><td class="a">${v.slot != null ? "slot " + v.slot : ""}</td>` +
              `<td class="nm">${esc(v.name)} <i>${esc(v.type || "")}${v.arg ? " · arg" : ""}</i></td>` +
              `<td class="v">${valueCell(v.value)}</td></tr>`
            );
          })
          .join("") +
        "</table>"
      : '<div class="note">sem variáveis locais visíveis neste ponto</div>';
    html += "</div>";
  }
  html += "</div>";

  html += `<div class="region heap"><div class="rh">Heap <small>objetos e vetores alcançáveis pelas variáveis (id do JDWP)</small></div>`;
  const heap = st.heap || [];
  html += heap.length
    ? '<table class="vars">' +
      heap
        .map((h) => {
          const chg = changed("h:" + h.id, h.value);
          return (
            `<tr class="${chg ? "chg" : ""}"><td class="a">#${h.id}</td>` +
            `<td class="nm">${esc(h.type)} <i>← ${esc(h.from)}</i></td><td class="v">${esc(h.value)}</td></tr>`
          );
        })
        .join("") +
      "</table>"
    : '<div class="note">nenhum objeto referenciado no momento</div>';
  html +=
    '<div class="note">A JVM não expõe endereços reais: os objetos são identificados pelo id do depurador. Strings aparecem diretamente pelo valor.</div></div>';

  html += `<div class="region methods"><div class="rh">Área de métodos <small>campos static das classes</small></div>`;
  const statics = st.statics || [];
  html += statics.length
    ? '<table class="vars">' +
      statics
        .map((s) => {
          const chg = changed(`s:${s.class}.${s.name}`, s.value);
          return (
            `<tr class="${chg ? "chg" : ""}"><td class="a">${esc(s.class)}</td>` +
            `<td class="nm">${esc(s.name)} <i>${esc(s.type || "")}</i></td><td class="v">${valueCell(s.value)}</td></tr>`
          );
        })
        .join("") +
      "</table>"
    : '<div class="note">nenhum campo static</div>';
  html += "</div>";

  const scroll = el.memView.scrollTop;
  el.memView.innerHTML = html;
  el.memView.scrollTop = scroll;
}

// ======================================================================
//  Navegação / reprodução
// ======================================================================
function go(i) {
  if (!S.steps.length) return;
  S.idx = Math.max(0, Math.min(S.steps.length - 1, i));
  render();
}
const groupKey = (i) => {
  const s = S.steps[i];
  return `${s.frames.length}:${s.func}:${s.line}`;
};

function nextLine() {
  const n = S.steps.length;
  if (!n) return false;
  const k = groupKey(S.idx);
  let j = S.idx + 1;
  while (j < n && groupKey(j) === k) j++;
  if (j >= n) {
    go(n - 1);
    return false;
  }
  go(j);
  return true;
}

function prevLine() {
  if (S.idx <= 0) return;
  let j = S.idx - 1;
  const k = groupKey(j);
  while (j > 0 && groupKey(j - 1) === k) j--;
  go(j);
}

function stopPlay() {
  if (S.timer) clearInterval(S.timer);
  S.timer = null;
  el.bPlay.textContent = "▶";
}

function togglePlay() {
  if (S.timer) return stopPlay();
  if (!S.steps.length) return;
  if (S.idx >= S.steps.length - 1) go(0);
  el.bPlay.textContent = "⏸";
  S.timer = setInterval(() => {
    const moved =
      el.playMode.value === "line"
        ? nextLine()
        : S.idx < S.steps.length - 1 && (go(S.idx + 1), true);
    if (!moved || S.idx >= S.steps.length - 1) stopPlay();
  }, +el.speed.value);
}

$("#bFirst").onclick = () => go(0);
$("#bLast").onclick = () => go(S.steps.length - 1);
$("#bPrev").onclick = () => go(S.idx - 1);
$("#bNext").onclick = () => go(S.idx + 1);
$("#bPrevLine").onclick = prevLine;
$("#bNextLine").onclick = nextLine;
el.bPlay.onclick = togglePlay;
el.speed.oninput = () => {
  if (S.timer) {
    stopPlay();
    togglePlay();
  }
};
el.timeline.oninput = () => go(+el.timeline.value);

document.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
    e.preventDefault();
    build(e.shiftKey);
    return;
  }
  const tag = document.activeElement && document.activeElement.tagName;
  if (tag === "TEXTAREA" || tag === "INPUT" || tag === "SELECT") return;
  const actions = {
    ArrowRight: () => go(S.idx + 1),
    ArrowLeft: () => go(S.idx - 1),
    ArrowDown: nextLine,
    ArrowUp: prevLine,
    Home: () => go(0),
    End: () => go(S.steps.length - 1),
    " ": togglePlay,
  };
  if (actions[e.key] && S.steps.length) {
    e.preventDefault();
    actions[e.key]();
  }
});

// ======================================================================
//  Abas e opções
// ======================================================================
function syncTabs() {
  for (const b of $$("#asmTabs button"))
    b.classList.toggle("active", b.dataset.tab === S.asmTab);
  for (const b of $$("#memTabs button"))
    b.classList.toggle("active", b.dataset.tab === S.memTab);
}
for (const b of $$("#asmTabs button"))
  b.onclick = () => {
    S.asmTab = b.dataset.tab;
    syncTabs();
    renderAsm();
  };
for (const b of $$("#memTabs button"))
  b.onclick = () => {
    S.memTab = b.dataset.tab;
    syncTabs();
    renderStep();
  };
el.showDir.onchange = renderAsm;

for (const [node, key] of [
  [el.arch, "arch"],
  [el.syntax, "syntax"],
  [el.opt, "opt"],
]) {
  node.value = store.get("asmviz." + key, node.value);
  node.addEventListener("change", () => {
    store.set("asmviz." + key, node.value);
    applyLangUI();
    if (S.result) build(false); // recompila com a nova configuração
  });
}

el.examples.onchange = () => {
  const code = EXAMPLES[S.lang][el.examples.value];
  if (!code) return;
  el.src.value = code;
  store.set("asmviz.code." + S.lang, code);
  setLang(S.lang, false);
  updateGutter();
};

el.btnCompile.onclick = () => build(false);
el.btnRun.onclick = () => build(true);

// ======================================================================
//  Início
// ======================================================================
setLang(
  EXAMPLES[store.get("asmviz.lang", "c")] ? store.get("asmviz.lang", "c") : "c",
);
loadTools();
