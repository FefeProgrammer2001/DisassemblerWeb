# Visualizador C/C++/Rust → Assembly e Java → Bytecode
#
#   docker compose up -d --build        (ou: docker build -t asmviz . && docker run ...)
#
# Base Arch Linux: mesmos pacotes e caminhos do ambiente de desenvolvimento
# (sysroot ARM64 em /usr/aarch64-linux-gnu, gdb com suporte a AArch64...).
# A imagem é x86_64 (a imagem oficial do Arch só existe para amd64).

# ---------------------------------------------------------------------------
# Ferramentas usadas em tempo de execução (compiladores, depuradores, qemu, JDK)
# ---------------------------------------------------------------------------
FROM archlinux:latest AS tools

RUN pacman -Syu --noconfirm --needed \
        gcc clang lld llvm gdb \
        aarch64-linux-gnu-gcc aarch64-linux-gnu-glibc qemu-user \
        jdk-openjdk \
        rustup \
    && pacman -Scc --noconfirm \
    && rm -rf /var/cache/pacman/pkg/* /var/lib/pacman/sync/*

# Rust instalado fora do $HOME para o usuário sem privilégios usar
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH
RUN rustup set profile minimal \
    && rustup default stable \
    && rustup target add aarch64-unknown-linux-gnu \
    && rm -rf /opt/rustup/downloads /opt/rustup/tmp

# ---------------------------------------------------------------------------
# Compilação do servidor
# ---------------------------------------------------------------------------
FROM tools AS build
WORKDIR /src
# dependências primeiro, para aproveitar o cache das camadas
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src target/release/asmviz* target/release/deps/asmviz-* target/release/.fingerprint/asmviz-*
COPY src ./src
RUN cargo build --release --locked

# ---------------------------------------------------------------------------
# Imagem final
# ---------------------------------------------------------------------------
FROM tools
# usuário sem privilégios: o servidor compila e executa código arbitrário
RUN useradd --create-home --uid 1000 asmviz
WORKDIR /app
COPY --from=build /src/target/release/asmviz /app/asmviz
COPY static /app/static

ENV STATIC_DIR=/app/static \
    HOST=0.0.0.0 \
    PORT=36476 \
    MAX_JOBS=4 \
    LANG=C.UTF-8

USER asmviz
EXPOSE 36476
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD curl -fsS "http://127.0.0.1:${PORT}/api/tools" >/dev/null || exit 1
CMD ["/app/asmviz"]
