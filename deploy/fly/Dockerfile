FROM node:24-bookworm-slim AS site
WORKDIR /site
COPY website/package.json website/package-lock.json ./
RUN npm ci
COPY website/src src
COPY website/static static
COPY website/scripts scripts
COPY website/vite.config.ts website/tsconfig.json website/sw-template.js website/background-sync.js ./
RUN npm run check && npm run build

FROM caddy:2
COPY --from=site /site/dist /srv/yap-site
COPY netkit/peering/covenant-v1.txt /srv/yap-site/peering/v1.txt
COPY netkit/peering/covenant-v2.txt /srv/yap-site/peering/v2.txt
COPY deploy/fly/Caddyfile /etc/caddy/Caddyfile
