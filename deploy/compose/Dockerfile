FROM node:24-bookworm-slim AS admin-ui
WORKDIR /site
COPY website/package.json website/package-lock.json ./
RUN npm ci
COPY website/vite.admin.config.ts website/tsconfig.json ./
COPY website/src/app.html src/app.html
COPY website/src/admin/ src/admin/
COPY website/src/lib/ActionButton.svelte src/lib/ActionButton.svelte
COPY website/src/routes/app/skeleton.css src/routes/app/skeleton.css
COPY website/scripts/ui-notices.mjs scripts/ui-notices.mjs
COPY website/static/app/fonts/geist-sans.woff2 website/static/app/fonts/OFL.txt static/app/fonts/
RUN npm run build:admin

FROM rust:1.93.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock LICENSING.md ./
COPY backend/ backend/
COPY netkit/ netkit/
ARG YAP_BUILD_SOURCE_SHA256
RUN cargo build --locked --release --bin yappers-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 yap \
    && useradd --uid 10001 --gid yap --no-create-home --home-dir /var/lib/yap/home yap \
    && install -d -m 0700 -o yap -g yap /var/lib/yap/home
RUN install -d -m 0700 -o yap -g yap /var/lib/yap-reports
COPY --from=build /src/target/release/yappers-server /usr/local/bin/yappers-server
COPY --from=admin-ui /site/dist-admin /usr/local/share/yap/admin
WORKDIR /var/lib/yap/home
USER yap
