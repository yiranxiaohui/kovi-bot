### 前端构建阶段
FROM oven/bun:1 AS fe
WORKDIR /fe
COPY plugins/panel/frontend/ .
RUN bun install && bun run build

### 构建阶段
FROM rust:bookworm AS builder
WORKDIR /app
COPY . .
# 注入前端产物,供 rust-embed 编译期嵌入(覆盖占位 dist/index.html)
COPY --from=fe /fe/dist ./plugins/panel/frontend/dist
RUN cargo build --release

# 运行阶段：使用 Distroless（含 glibc）
FROM debian:bookworm-slim

# 安装 TLS 和字体渲染所需的运行时依赖
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    fontconfig \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN update-ca-certificates

# 直接复制二进制（Distroless 已含 glibc 和 ca-certificates）
COPY --from=builder /app/target/release/kovi-bot /app/kovi-bot

COPY --from=builder /app/kovi.conf.toml /app/kovi.conf.toml
COPY --from=builder /app/kovi.plugin.toml /app/kovi.plugin.toml

WORKDIR /app

ENTRYPOINT ["./kovi-bot"]
