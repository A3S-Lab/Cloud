# Cloud Web Gateway profile

This profile exposes the Cloud management SPA and API through one A3S Gateway
origin:

- `/api` and `/api/*` route to the control plane on `127.0.0.1:8080`;
- every other path routes to the production SPA server on `127.0.0.1:3011`;
- the SPA server owns history fallback, static content types, cache policy, and
  browser security headers; and
- Gateway owns the public listener, request routing, observability, and the TLS
  entrypoint when an operator adds the deployment certificate.

Build the immutable browser assets and start their private upstream:

```bash
cd web
bun install --frozen-lockfile
bun run build
cd ..
cargo run -p a3s-cloud-web-server -- \
  --listen 127.0.0.1:3011 \
  --root web/dist
```

With the control plane listening on `127.0.0.1:8080`, validate and start the
Gateway profile:

```bash
a3s-gateway validate --config deploy/web/gateway.acl
a3s-gateway --config deploy/web/gateway.acl
```

The local validation origin is `http://127.0.0.1:8088`. The shipped profile is
loopback-only so it cannot accidentally publish an unauthenticated development
installation. Before binding a non-loopback address, add an operator-owned TLS
block to the `cloud` entrypoint and keep both upstream services private.
