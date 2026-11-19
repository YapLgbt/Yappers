FROM rust:1.93.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml LICENSING.md fly.toml .dockerignore ./
COPY LICENSES LICENSES
COPY deploy deploy
COPY backend backend
COPY netkit netkit
RUN sh deploy/peering/package-source.sh /out && YAP_BUILD_SOURCE_SHA256="$(cat /out/revision.txt)" cargo +1.93.1 build --locked --release --bin yappers-server

FROM node:24-bookworm-slim AS site
WORKDIR /site
COPY website/package.json website/package-lock.json ./
RUN npm ci
COPY website/src src
COPY website/static static
COPY website/scripts scripts
COPY website/vite.config.ts website/tsconfig.json website/sw-template.js website/background-sync.js ./
RUN npm run check && npm run build

FROM node:24-bookworm-slim AS push
WORKDIR /opt/yap-push
COPY deploy/push/package*.json ./
RUN npm ci --omit=dev --ignore-scripts
COPY deploy/push/send.cjs ./

FROM caddy:2 AS web
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libstdc++6 libatomic1 && rm -rf /var/lib/apt/lists/*
COPY --from=push /usr/local/bin/node /usr/local/bin/node
COPY --from=push /opt/yap-push /opt/yap-push
COPY --from=web /usr/bin/caddy /usr/local/bin/caddy
COPY --from=build /src/target/release/yappers-server /usr/local/bin/yappers-server
COPY --from=site /site/dist /srv/yap-site
COPY --from=build /out /srv/yap-site/source
COPY --from=build /out/peering.json /srv/yap-site/.well-known/yap-peering.json
COPY netkit/peering/covenant-v1.txt /srv/yap-site/peering/v1.txt
COPY deploy/peering/yap.json /etc/yap/peering.json
COPY deploy/fly/Caddyfile /etc/caddy/Caddyfile
COPY deploy/fly/start.sh /usr/local/bin/start-yap
ENTRYPOINT ["/usr/local/bin/start-yap"]
