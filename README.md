# Código → Máquina: C / C++ / Rust → Assembly e Java → Bytecode

Você digita código **C**, **C++**, **Rust** ou **Java**, o site compila, mostra o código de
máquina gerado e executa o programa passo a passo:

| Linguagem | Compilador | O que é mostrado | Execução passo a passo |
|-----------|-----------|------------------|------------------------|
| C         | `clang`   | Assembly x86_64 ou ARM64 (`clang -S` e desmontagem do binário) | gdb (x86_64) / qemu-aarch64 + gdb (ARM64), instrução a instrução |
| C++       | `clang++` | Idem, com nomes "demangled" (`Contador::inc(int)`) | Idem |
| Rust      | `rustc`   | Assembly das funções do programa (`rustc --emit asm` e desmontagem do binário), nomes como `prog::main` | Idem (gdb), a partir de `prog::main` |
| Java      | `javac -g`| Bytecode da JVM (`javap -c`) | jdb, instrução de bytecode a instrução (`stepi`) |

A cada passo o site destaca a linha do código-fonte, a instrução atual e o estado da memória:

- **C/C++/Rust**: registradores, frames da pilha com variáveis (endereço, tipo, valor),
  heap apontado por ponteiros, globais/estáticas, funções no `.text` e os bytes brutos
  da pilha (aba "Pilha (bytes)").
  A aba "Pilha (visual)" desenha cada chamada como um bloco, com variáveis, `rbp`/`x29`
  salvo, endereço de retorno (e a linha para onde volta), espaços de alinhamento, red
  zone, os marcadores de `rsp`/`rbp` e setas para ponteiros e para a cadeia de frames.
- **Java**: frames da JVM com as variáveis locais por *slot*, objetos e vetores no heap
  (identificados pelo id do depurador) e os campos `static` (área de métodos).

## Estrutura

```
Cargo.toml
src/
  main.rs     servidor HTTP (tiny_http), rotas e configuração
  native.rs   C/C++: clang, llvm-objdump, llvm-nm e trace via GDB/MI
  rust.rs     Rust: rustc (reaproveita o parser e o trace do native.rs)
  session.rs  entrada interativa: execuções pausadas esperando o stdin
  mi.rs       cliente e parser do protocolo GDB/MI
  java.rs     Java: javac, javap e trace via jdb
  util.rs     processos com tempo limite, diretório temporário, etc.
static/
  index.html  interface (HTML + CSS)
  app.js      lógica da interface
```

O navegador não executa compiladores, por isso há o servidor em Rust. O gdb é
controlado pela interface **GDB/MI** e o jdb pela entrada/saída de texto, sem scripts
Python.

## Dependências (Arch / CachyOS)

```sh
# Rust
sudo pacman -S rustup && rustup default stable

# C / C++ (x86_64)
sudo pacman -S clang lld llvm gdb

# ARM64 (cross compiling + execução emulada)
sudo pacman -S aarch64-linux-gnu-gcc aarch64-linux-gnu-glibc qemu-user

# Rust: o rustc do rustup (já instalado acima); para ARM64:
rustup target add aarch64-unknown-linux-gnu

# Java (javac, javap, java, jdb)
sudo pacman -S jdk-openjdk
```

- `aarch64-linux-gnu-glibc` fornece o sysroot (`/usr/aarch64-linux-gnu`) com headers e
  libc ARM64; `aarch64-linux-gnu-gcc` fornece `crtbegin.o`, `libgcc` e `libstdc++` ARM64.
- O `gdb` do Arch já suporta AArch64 (não é preciso `gdb-multiarch`).

## Docker

A imagem já traz todas as ferramentas (clang, lld, llvm, gdb, cross ARM64, qemu-user,
JDK e Rust com o alvo ARM64). Ela é x86_64 e tem alguns GB.

```sh
docker compose up -d --build        # http://localhost:36476
docker compose logs -f              # endereços e requisições
docker compose down
```

Sem o compose:

```sh
docker build -t asmviz .
docker run -d --name asmviz -p 36476:36476 \
  --read-only --tmpfs /tmp:exec,size=1g \
  --cap-drop ALL --security-opt no-new-privileges \
  --pids-limit 512 --memory 3g --cpus 2 \
  asmviz
```

O `docker-compose.yml` aplica as mesmas restrições: usuário sem privilégios, sistema de
arquivos somente leitura (o trabalho fica no `/tmp` em memória, montado com `exec`
porque os programas compilados rodam de lá), sem capabilities e com limites de
processos, memória e CPU. Variáveis da tabela abaixo (`MAX_JOBS`, `PORT`...) podem ser
passadas em `environment:`; ao mudar `PORT`, ajuste também `ports:`.

O perfil seccomp padrão do Docker não permite ao gdb desligar o ASLR (`personality`):
ele só avisa e segue. Como os binários são estáticos e sem PIE, os endereços de código
não mudam; só os da pilha e do heap variam entre execuções.

## Executar

```sh
cargo run --release
```

Por padrão o servidor escuta em **todas as interfaces** (`0.0.0.0:36476`) e mostra no
terminal o endereço local e o da rede, por exemplo:

```
Visualizador C/C++/Rust/Java escutando em 0.0.0.0:36476
  local: http://127.0.0.1:36476
  rede:  http://192.168.0.10:36476
```

Se outra máquina não conseguir acessar, libere a porta no firewall (ex.: `sudo ufw allow 36476/tcp`
ou o equivalente do firewalld).

### Variáveis de ambiente

| Variável | Padrão | Uso |
|----------|--------|-----|
| `HOST` | `0.0.0.0` | Interface de escuta (`127.0.0.1` = só local, `::` = IPv6) |
| `PORT` | `36476` | Porta |
| `MAX_JOBS` | `4` | Compilações/execuções simultâneas |
| `STATIC_DIR` | `<projeto>/static` | Pasta do frontend |
| `CLANG`, `CLANGXX`, `LLVM_OBJDUMP`, `LLVM_NM`, `LLVM_CXXFILT`, `GDB` | nomes padrão | Caminhos das ferramentas |
| `AARCH64_SYSROOT` | `/usr/aarch64-linux-gnu` | Sysroot ARM64 |
| `QEMU_AARCH64` | `qemu-aarch64` no PATH | Emulador ARM64 |
| `RUSTC` | `rustc` | Compilador Rust |
| `JAVAC`, `JAVAP`, `JAVA`, `JDB` | nomes padrão | Ferramentas Java |
| `ASMVIZ_DEBUG` | — | Se definida, registra no terminal a conversa com o jdb |

## Comandos usados

C/C++ x86_64 (nativo):

```sh
clang   --target=x86_64-linux-gnu -O0 -g -std=gnu17  -fno-omit-frame-pointer -fno-stack-protector -S prog.c
clang++ --target=x86_64-linux-gnu -O0 -g -std=gnu++20 ... -S prog.cpp
clang   ... prog.c nobuf.c -o prog -fuse-ld=lld -static -lm
llvm-objdump -d -l --no-show-raw-insn --disassemble-symbols=<funções do usuário> prog
gdb --interpreter=mi3   (stepi a partir de main)
```

C/C++ ARM64 (cross compiling):

```sh
clang --target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu -O0 -g ... -S prog.c
clang --target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu ... -fuse-ld=lld -static -lm -o prog
qemu-aarch64 -g <porta> ./prog   +   gdb --interpreter=mi3 (target remote)
```

Rust (x86_64; para ARM64 acrescenta `--target aarch64-unknown-linux-gnu -C linker=aarch64-linux-gnu-gcc`
e executa com qemu como no C):

```sh
rustc --edition 2024 --crate-name prog -g -C opt-level=0 -C force-frame-pointers=yes -C codegen-units=1 \
      -C symbol-mangling-version=v0 -C relocation-model=static -C target-feature=+crt-static \
      --emit=asm=prog.s,link=prog prog.rs
llvm-objdump -d -l --no-show-raw-insn --disassemble-symbols=<funções do usuário> prog
gdb --interpreter=mi3   (stepi a partir de prog::main)
```

Java:

```sh
javac -g -encoding UTF-8 -d classes Main.java
javap -c -l -p -constants -cp classes <classes>
java -agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:<porta> -cp classes Main
jdb -attach 127.0.0.1:<porta>    (stepi)
```

O stdin do programa é um FIFO (`stdin.fifo`) cuja ponta de escrita fica com o servidor.
O trace roda numa thread própria; enquanto espera o programa (dentro de uma função de
biblioteca), o servidor consulta `/proc/<pid>/task/*/syscall` e, se alguma thread está
num `read` do fd 0 (no programa, no `qemu-aarch64` ou na JVM), devolve ao navegador os
passos até ali com status `input`. O texto digitado chega por `POST /api/input`
(`{session, text}`, `{session, eof: true}` ou `{session, cancel: true}`), é escrito no
FIFO e a execução continua; o tempo de espera não conta para o limite do trace e uma
execução pausada é encerrada após 15 minutos sem resposta.

O `nobuf.c` é ligado junto ao programa C/C++ para desligar o buffer do `stdout`, assim
a saída aparece exatamente no passo em que foi produzida.

## Uso

- **Compilar** (Ctrl+Enter): só gera o assembly/bytecode. Passe o mouse sobre uma linha
  do código para ver as instruções correspondentes (e vice-versa).
- **Executar passo a passo** (Ctrl+Shift+Enter): compila, executa e grava o trace.
- Navegação: `→`/`←` instrução, `↓`/`↑` linha do código, `Home`/`End`, `espaço` reproduz.
- **Entrada do usuário**: quando o programa chama uma função que lê o stdin (`scanf`,
  `cin >>`, `getline`, `read_line`, `Scanner.nextInt`...) e não há dados, a execução
  pausa e aparece um campo abaixo da "Saída do programa". Digite o valor e pressione
  Enter (o texto é enviado com `\n` e ecoado na saída, como num terminal); **EOF**
  (ou Ctrl+D no campo) fecha a entrada. O campo "Entrada do programa (stdin)" continua
  servindo para entregar a entrada de antemão; só o que faltar é pedido.

## Limitações

- Chamadas de biblioteca (`printf`, `malloc`, `std::cout`, `System.out.println`...)
  são executadas de uma vez, sem entrar nelas.
- Rust: código da std é executado de uma vez, inclusive closures chamadas por ela
  (ex.: o `|x| x * x` de `iter().map(...)` não é acompanhado; uma closure chamada
  diretamente pelo programa é). Funções da std que o compilador expande inline
  (`vec!`, `Box::new`) aparecem como instruções da linha que as usa. O `stdout` do
  Rust é bufferizado por linha: um `print!` sem `\n` só aparece no próximo `println!`.
  ARM64 exige `rustup target add aarch64-unknown-linux-gnu`.
- C/C++: funções de templates da biblioteca padrão (ex.: `std::vector`) são tratadas
  como biblioteca; só a pilha até 4 KB e 64 bytes de cada região apontada são mostrados;
  com `-O1`+ muitas variáveis ficam `<optimized out>`.
- Java: a pilha de operandos não é exposta pelo JDWP/jdb; vetores com mais de 64
  elementos não têm o conteúdo listado; programas multi-thread acompanham só a thread `main`.
- O trace para no limite de passos (padrão 3000, máximo 20000) ou após ~100 s. O Java é
  mais lento por passo (cada passo faz várias consultas ao jdb).

## Segurança

O servidor **compila e executa qualquer código recebido**, com as permissões do usuário
que o iniciou. Escutar em `0.0.0.0` é apropriado apenas para testes em uma rede
confiável. Para uso só local, rode com `HOST=127.0.0.1 cargo run --release`.
