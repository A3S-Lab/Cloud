# C0.1 Cross-Surface Conformance

This gate proves the shipped REST v1 contract, the exact shared `CloudApi`
import used by the Web console, and the compiled `a3s-cloud` CLI against one
real Cloud control-plane process and PostgreSQL 17 database.

The scenario:

1. waits for public liveness and readiness through the Web client import;
2. bootstraps the first organization through raw REST without returning the
   bootstrap or administrator credential;
3. creates a Project through the Web client and replays the same idempotency
   identity through the compiled CLI;
4. creates an Environment through raw REST, replays it through the CLI, and
   reads it through the Web client;
5. compares authorized search results from the Web client and CLI;
6. verifies stable CLI conflict, cross-tenant denial, and revoked-token error
   contracts;
7. verifies revocation takes effect on the next request; and
8. dumps the real PostgreSQL data and rejects plaintext bootstrap, API-token,
   webhook, or database credentials while requiring the expected token digests
   and durable revocation row.

The runner uses the shipped `config/cloud.acl`, parses it through the normal
`a3s-acl` configuration boundary, and executes production PostgreSQL
repositories through A3S ORM. It creates isolated temporary state and a
digest-pinned PostgreSQL container, then removes both on exit.

Run it from a clean Cloud checkout whose sibling Runtime checkout matches
`tools/runtime-conformance/runtime-revision`:

```bash
evidence="$(mktemp -d)"
tools/c0-conformance/run_cross_surface_gate.sh "$evidence"
cat "$evidence/result.txt"
```

Ports `127.0.0.1:8080` and `127.0.0.1:8443` must be available because the gate
verifies the shipped development ACL unchanged. A local pre-commit rehearsal
may set `A3S_CLOUD_C0_ALLOW_DIRTY=true`; its result is explicitly marked
`dirty-rehearsal` and is not release evidence.

A passing clean run writes
`A3S_CLOUD_C0_1_CROSS_SURFACE_PASS` with the exact Cloud and Runtime revisions,
the REST contract version, sanitized scenario evidence, provider logs, and the
credential-free persistence check.
