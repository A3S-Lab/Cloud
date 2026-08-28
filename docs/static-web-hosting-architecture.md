# A3S Cloud Static Web Hosting Architecture

## 1. Decision

A3S Cloud will support React, Vue, Svelte, and other static front-end projects
as immutable Web releases. A static site is **built by a Runtime Task on A3S
Box, stored in the shared S3-compatible object authority, and served publicly
by A3S Gateway**. It is not a long-running Runtime Service.

SSR, BFF, WebSocket, and server-side framework deployments remain ordinary
Workload/Runtime Services. A hybrid application may bind one static Web release
and one or more Service/FaaS API routes behind the same Gateway origin.

This is the target `WEB0` architecture. Gateway does not currently implement
the required static object target, so Web hosting is unavailable until the
named gates pass.

## 2. Why this shape

A static site needs four things: trusted source identity, an isolated build,
immutable bytes, and public HTTP delivery. It does not need placement,
health-check, restart, replica, or process recovery per release.

Running Nginx or a framework dev server for every site would duplicate object
serving, consume idle capacity, complicate rollouts, and make static availability
depend on a process. Letting browsers access S3 directly would bypass the single
Gateway ingress, tenant policy, domains, TLS, headers, audit, and revocation.

The minimal path is therefore:

```text
Hosted Git / admitted SourceRevision
  -> Developer Workflows accepted BuildPlan
  -> Artifacts BuildRun
  -> Runtime Task -> A3S Box sandbox
  -> verified immutable Web bundle in shared S3
  -> Assets WebRelease + Applications presentation binding
  -> Edge complete static-target snapshot
  -> A3S Gateway read-only object target + bounded cache
  -> browser
```

## 3. DDD ownership

No new bounded context is created for the first slice.

| Concern | Sole owner |
| --- | --- |
| Hosted repository and immutable Web release identity | Assets; planned `web` Asset kind |
| External repository connection and exact revision | Sources |
| Build admission, sandboxed BuildRun, provenance and verified output | Developer Workflows + Artifacts |
| Build process lifecycle | Executions + A3S Runtime Task + A3S Box |
| Immutable bundle bytes | One deployment object authority and a typed Web namespace |
| Application-to-UI binding | Applications immutable release/presentation binding |
| Domain, TLS, route, authorization and desired target | Edge |
| Public GET/HEAD delivery, cache, compression and security headers | A3S Gateway |
| Preview identity, quota and expiry | Existing Developer Workflows Preview authority |
| Secrets | Secrets; never source files, build logs, bundle manifest, snapshot, or browser bytes |

Assets gains a closed Web release kind only after its invariants and migrations
are frozen. A parallel `StaticSites` repository, build queue, deployer, object
client, route publisher, or cleanup scheduler is prohibited.

## 4. Build contract

The planned `cloud.web.build.v1` ACL is framework-neutral. React and Vue are
toolchain detections, not domain types. It pins:

- exact source revision and repository root;
- lockfile digest and package-manager/toolchain revision;
- sandbox image digest, build command and bounded working directory;
- environment **references**, never plaintext values;
- CPU, memory, PIDs, ephemeral storage, network/egress and timeout policy;
- output directory, maximum file count, maximum file and bundle sizes; and
- expected build-output media type and manifest schema.

The build runs as a finite Runtime Task on Box. Dependency access is denied or
allowlisted by the accepted policy. A missing lockfile, mutable toolchain tag,
path escape, symlink escape, oversized output, secret-bearing output, or
unverified digest fails before publication.

## 5. Immutable Web bundle

Artifacts verifies and publishes one content-addressed bundle plus one canonical
manifest. Domain records keep logical object references and digests, never a
bucket credential or mutable provider URL.

The manifest contains bounded entries:

```text
path -> sha256 digest + size + canonical media type + content encoding
```

It also binds the release digest, entry document, base path, optional SPA
fallback, and build provenance digest. Paths are normalized UTF-8 relative
paths. Absolute paths, `..`, backslashes, NULs, ambiguous percent encoding,
duplicate normalized paths, symlinks, device files, and case-colliding paths
are rejected.

Publication is create-only. Rebuilding identical source and policy may adopt
the same digest. Reusing a release identity for different bytes is forbidden.

## 6. Gateway static target

Edge compiles a new closed `static_bundle` target into the complete Gateway
snapshot. It contains only:

- organization/project/environment and Route identity;
- exact Web release, manifest and bundle digests;
- logical object namespace/reference;
- base path and explicit SPA fallback policy;
- authentication, response-header, cache, compression and request-limit
  policy; and
- snapshot revision, expiry and rollout/drain evidence.

No S3 endpoint or credential appears in the snapshot. Gateway owns one
read-only static-object port backed by the same deployment object authority. It
may perform only digest-bound `HEAD`/`GET`; it cannot list, write, delete,
restore, or infer a current release.

Gateway validates the snapshot and manifest before activation, then serves only
`GET` and `HEAD`:

- normalize and decode the request path once; reject traversal and ambiguity;
- select a manifest entry before object access;
- verify size and digest before admitting an object to cache;
- derive `Content-Type` from the sealed manifest, set `nosniff`, and apply the
  route's CSP and other security headers;
- use a short/no-cache policy for the entry document and immutable long caching
  only for digest-named assets;
- namespace cache keys by tenant, release digest, manifest digest, encoding and
  path;
- support conditional requests and bounded byte ranges only after separate
  tests; and
- purge or make old cache entries unreachable by snapshot replacement rather
  than mutable in-place content.

SPA fallback is opt-in and applies only to eligible `GET`/`HEAD` navigation
requests under the static route. It never masks a missing asset, `/api` route,
method error, authorization failure, or object-integrity failure.

## 7. Agent UI composition

An immutable Application release may bind:

- one Web release for `/`;
- one Agent/Application API route under `/api`;
- SSE or WebSocket routes for semantic events; and
- optional Function, Workflow, MCP, inference, or Durable Cell routes.

Gateway presents one origin, TLS policy, authentication session, request ID,
tenant attribution and security-header policy. The browser never receives a
Runtime endpoint, Box identity, object credential, external FaaS credential, or
unscoped Agent token.

The UI binding is presentation intent only. It cannot mutate AgentExecution or
WorkflowRun state, infer authorization from a route, or become an execution
authority.

## 8. Static, SSR, and hybrid decision

| Project output | Deployment |
| --- | --- |
| HTML/CSS/JS/images/fonts only | Web bundle in S3, served by Gateway; no Service |
| Pre-rendered site with SPA/client hydration | Same static path |
| SSR or BFF HTTP server | Workload/Runtime Service behind Gateway |
| Finite server-side render or export job | Runtime Task; publish static output if immutable |
| API/webhook Function | FaaS Task or Service profile |
| WebSocket or durable session owner | Ordinary Service; Durable Cell only when named-state semantics are explicitly required |

Framework auto-detection may propose a BuildPlan, but a tenant-authorized user
must accept the immutable plan before execution. Detection never becomes an
unversioned deployment decision.

## 9. Multi-tenancy, quotas, and cleanup

- Every release, object reference, cache key, Route and audit record carries
  canonical organization/project/environment attribution.
- Build CPU/time/output and hosted-byte quotas are reserved before work.
- Gateway object credentials are read-only and deployment-scoped; object keys
  remain unreachable without an admitted Route snapshot.
- Preview releases use the existing Preview identity, TTL and cleanup owner.
- Release retirement first removes/drains the Gateway snapshot. Object garbage
  collection occurs later through the sole object-retention worker after every
  live release/reference fence is checked.
- A cache hit never bypasses current snapshot authorization or expiry.

## 10. `WEB0` delivery gates

| Gate | Outcome |
| --- | --- |
| `WEB0.1` | Freeze Web Asset/release, build/profile ACL, immutable manifest, Application UI binding, static target and status/error contracts |
| `WEB0.2` | Build pinned React and Vue fixtures through the existing Developer Workflows/Artifacts/Execution/Runtime Task/Box path with provenance, bounds, cancellation, replay and cleanup evidence |
| `WEB0.3` | Publish verified bundles through the one S3 object authority with create-only replay, quota, retention, corruption and external HTTPS S3 evidence |
| `WEB0.4` | Add Gateway's closed read-only static target, manifest/digest validation, cache, MIME, headers, SPA fallback, drain and traversal/tenant security tests |
| `WEB0.5` | Bind Web releases to Applications/Agents and Preview routes through Edge with REST/client/CLI/Management MCP management surfaces |
| `WEB0.6` | Run exact Gateway/Cloud/Runtime/Box/S3 end-to-end, upgrade/rollback, process/node loss, load, accessibility smoke, browser security and zero-residue gates |

`WEB0` may proceed after the Runtime/Box build substrate and object authority
are certified. It does not depend on Durable Cell or model inference.

## 11. Non-goals

- Reintroducing a product Web administration shell into this repository.
- Running a development server in production.
- One Nginx/container/Runtime Service per static release.
- Direct public S3 URLs or browser object credentials.
- Framework-specific Runtime classes or domain aggregates.
- Building source in Gateway, Edge, the API process, or an object-store hook.
- Letting Gateway list/mutate objects or discover the current release.
- Treating SSR, WebSocket, or stateful server code as static hosting.
