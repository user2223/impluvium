# No external frontend is needed.
ARG NODE_BASE=node:24-trixie-slim@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
ARG RUST_BASE=rust:1.99.1-slim-trixie@sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd
FROM ${NODE_BASE} AS base
RUN pnpm install \
    --frozen-lockfile --ignore-scripts
FROM base AS runtime
FROM ${RUST_BASE} AS compiler
ENV RUSTUP_TOOLCHAIN=1.99.1
RUN cargo build --release --locked -p core
