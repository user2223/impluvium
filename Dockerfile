# Server image. There is no "# syntax" line: the Dockerfile frontend built into the
# pinned BuildKit is used.
#
# Base images are pinned by digest here and only here (the tag is informational).
# All stages use Debian trixie, so the glibc of the build stages matches the runtime.
# The Rust version must match rust-toolchain.toml.
ARG NODE_BASE_IMAGE=node:24-trixie-slim@sha256:54f84b4cf8432b21aaffd5d3ce8d41e12cea0d10d33c09d2f3c9879d49d6b446
ARG RUST_BUILD_IMAGE=rust:1.99.1-slim-trixie@sha256:b8e062b1c3a80183df83d9ba187cadd2b5ab84bc78d453dbfeb2fccd5f982247

### Leak check of the build context. CI builds this target on its own, with a planted
### server/.env; the image build does not use it.
FROM ${NODE_BASE_IMAGE} AS context-audit
COPY . /ctx
RUN found="$(find /ctx -name '.env*' -print)"; \
    if [ -e /ctx/server/.env ] || [ -n "$found" ]; then \
      echo "context-audit: FAIL: .env files in the build context:"; echo "$found"; exit 1; \
    fi; \
    echo "context-audit: OK: no .env files in the build context"

### The build context (an allow-list, see .dockerignore) with pnpm from corepack
FROM ${NODE_BASE_IMAGE} AS node-base
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
## prevent prompt to download
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable
WORKDIR /app
COPY . .
## fetch the pnpm version named in package.json once, then stay offline
RUN corepack pack
ENV COREPACK_ENABLE_NETWORK=0

### Prisma CLI for the runtime image: a self-contained node_modules with exactly the
### versions that the lock file records for server/prisma-cli
FROM node-base AS prisma-cli
## openssl lets Prisma detect libssl 3 and load the debian-openssl-3.0.x schema engine;
## python3 runs the lock file comparison.
RUN apt-get update && apt-get install -y --no-install-recommends \
    openssl \
    ca-certificates \
    python3-minimal \
    && rm -rf /var/lib/apt/lists/*
## The package store goes into the package's own node_modules (pnpm links relatively
## inside it), so that directory works on its own wherever it is copied.
RUN pnpm install --frozen-lockfile --prod --filter "{./server/prisma-cli}" --fail-if-no-match \
    --virtual-store-dir server/prisma-cli/node_modules/.pnpm
RUN python3 -I server/prisma-cli/check-install.py pnpm-lock.yaml server/prisma-cli
## loads the schema engine now, at build time
RUN cd server/prisma-cli \
    && ./node_modules/.bin/prisma version | tee /tmp/prisma-version \
    && grep -q 'schema-engine-debian-openssl-3.0.x' /tmp/prisma-version

### Install the server's dependencies once
FROM node-base AS deps
## Only the server and its workspace dependencies (its Nuxt layer). The desktop projects are not in the build context.
RUN pnpm install --frozen-lockfile --ignore-scripts --filter "{./server}..." --fail-if-no-match

### Build torrential
FROM ${RUST_BUILD_IMAGE} AS torrential-build
## Use the image's toolchain instead of rust-toolchain.toml (which is not in the
## build context and would also install rustfmt and clippy).
ENV RUSTUP_TOOLCHAIN=1.99.1
## libarchive-dev + pkg-config let libarchive3-sys link libarchive dynamically (glibc).
## protobuf-compiler is kept for parity (torrential's build.rs uses a vendored protoc).
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libarchive-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
## The whole build context in one step (each COPY is slow under rootless BuildKit).
## Cargo loads every workspace member, so the desktop crates are needed as well (the
## context holds only their manifests and sources), but only torrential and its
## dependencies are built.
COPY . .
RUN cargo build --release --locked -p torrential

### Build the Nuxt app and collect the runtime files under /rootfs
FROM deps AS build-system

ENV NODE_ENV=production
ENV NUXT_TELEMETRY_DISABLED=1

ARG BUILD_DROP_VERSION
## The build context has no .git directory, so the git ref must be passed in
## (CI passes the commit); without it the build reports "unknown".
ARG BUILD_GIT_REF=unknown

RUN pnpm -C server run postinstall && pnpm -C server run build
RUN mkdir -p /rootfs/app \
    && cp -a server/.output /rootfs/app/app \
    && cp -a server/prisma /rootfs/app/prisma \
    && cp -a server/prisma.config.ts /rootfs/app/ \
    && cp -a server/build /rootfs/app/startup \
    && cp -a server/build/nginx.conf /rootfs/nginx.conf

### Runtime: the plain Node image plus the files copied below, no build context
FROM ${NODE_BASE_IMAGE} AS runtime

ENV NODE_ENV=production
ENV NUXT_TELEMETRY_DISABLED=1

## runtime deps:
##  - libarchive13t64: torrential links libarchive dynamically (glibc build)
##  - 7zip: kept for parity with the previous image (no code calls a 7z program)
##  - nginx: front-end proxy
##  - openssl + ca-certificates: required by Prisma's schema engine on Debian
RUN apt-get update && apt-get install -y --no-install-recommends \
    libarchive13t64 \
    7zip \
    nginx \
    openssl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

## /app holds the Nuxt output (app), the Prisma schema, migrations and configuration
## (prisma, prisma.config.ts), launch.sh (startup) and the Prisma CLI (node_modules; it
## also provides dotenv and prisma/config for prisma.config.ts).
## The server starts torrential from ../torrential (a source checkout), from a
## `torrential` entry in the working directory, from TORRENTIAL_PATH or from PATH.
## Neither /torrential nor /app/torrential exists here, so /usr/bin/torrential is used.
WORKDIR /app
COPY --from=build-system /rootfs/ /
COPY --from=prisma-cli /app/server/prisma-cli/node_modules ./node_modules
COPY --from=torrential-build /build/target/release/torrential /usr/bin/

ENV LIBRARY="/library"
ENV DATA="/data"
ENV NGINX_CONFIG="/nginx.conf"
# Nuxt's port
ENV PORT=4000

CMD ["sh", "/app/startup/launch.sh"]
