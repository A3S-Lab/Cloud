# C0 Cross-Surface Conformance

The C0 runner boots one real Cloud control-plane process and PostgreSQL 17
database from the shipped A3S ACL configuration. Scenario selection keeps the
verified `C0.1` REST/Web/CLI contract and the verified `C0.2` management MCP
contract independently runnable while sharing the same production fixture and
credential boundary.

## C0.1 REST, Web client, and CLI

The default scenario proves the shipped REST v1 contract, the exact shared
`CloudApi` import used by the Web console, and the compiled `a3s-cloud` CLI.
It:

1. waits for public liveness and readiness through the Web client import;
2. bootstraps the first organization through raw REST without returning the
   bootstrap or administrator credential;
3. creates a Project through the Web client and replays the same idempotency
   identity through the compiled CLI;
4. creates an Environment through raw REST, replays it through the CLI, and
   reads it through the Web client;
5. compares authorized search results from the Web client and CLI;
6. verifies stable CLI conflict, cross-tenant denial, and revoked-token error
   contracts; and
7. requires the expected Token digests, durable revocation, and zero plaintext
   credentials in API/CLI evidence or the PostgreSQL dump.

## C0.2m management MCP

The `management-mcp` scenario drives raw REST and stateless Streamable HTTP MCP
`2026-07-28` against the same production binary. It:

1. proves `server/discover`, mandatory per-request protocol/client metadata,
   matching transport headers, unsupported-version errors, complete-result
   metadata, and removal of the legacy initialization flow;
2. compares the 73-tool administrator and 45-tool `cloud:read` catalogs and
   verifies their behavioral annotations;
3. proves a hidden mutation cannot be invoked and leaves no Project row;
4. creates a Project through REST and replays the same command and idempotency
   key through MCP using one durable record;
5. creates an Ontology through REST, replays it through MCP, exercises all
   seven Ontology tools, rejects a breaking revision without its exact target
   migration rule, publishes the explicit migration, and proves historical
   replay after later revisions;
6. creates a native Form draft through REST, replays it through MCP, revises
   and publishes it through MCP, exercises all four read tools with the
   read-only token, and proves publication and historical revision replay;
7. creates one Environment, exercises Node, Operation, Workload, Route, and
   BuildRun lists, checks missing Node, Workload, Deployment, Route, and
   BuildRun details plus Workload logs, BuildRun logs, and BuildRun evidence,
   and rejects invalid list/log bounds, cursors, and stream filters;
8. checks all five replay-safe operational commands against missing resources,
   rejects missing, empty, and forged command arguments, then creates a
   Workload from A3S ACL and proves MCP stop plus exact replay;
9. rejects a forged organization argument and returns the same `404`
   business-error contract for a foreign and a missing Project;
10. revokes the read-only Token through REST and requires the next MCP request
   to return `401`; and
11. requires the expected Project, Ontology head, three immutable Ontology
    revisions, exact Ontology Outbox/audit/Search/idempotency rows, migration
    `075`, rejected-migration zero-write behavior, Form draft/release,
    Environment, stopped Workload, and Token-digest rows; foreign Ontology
    lookup is indistinguishable from missing, and responses, logs, evidence,
    and the PostgreSQL dump retain zero plaintext credentials.

Both scenarios execute production PostgreSQL repositories through A3S ORM.
The runner creates isolated temporary state and a digest-pinned PostgreSQL
Service in an A3S Box Sandbox, stores its database on tmpfs, and removes the
Box and state root on exit. Host access uses Box's loopback-only,
generation-fenced `port-forward` command; Sandbox static publication is not a
fallback path.

## Running the gates

Run on Linux from a clean Cloud checkout whose sibling Runtime checkout matches
`tools/runtime-conformance/runtime-revision`. Install the exact-revision Box
fixture or provide an equivalent `A3S_CLOUD_BOX_BIN` together with its exact
`A3S_CLOUD_BOX_REVISION`:

```bash
box_root="$(mktemp -d)"
tools/box-conformance/install_box_release.sh "$box_root"
export PATH="$box_root:$PATH"
export LD_LIBRARY_PATH="$box_root/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export A3S_BOX_OCI_RUNTIME_PATH="$box_root/a3s-oci"
export A3S_BOX_OCI_AGENT_PATH="$box_root/a3s-oci-agent"

c0_evidence="$(mktemp -d)"
tools/c0-conformance/run_cross_surface_gate.sh "$c0_evidence"
cat "$c0_evidence/result.txt"

mcp_evidence="$(mktemp -d)"
tools/c0-conformance/run_cross_surface_gate.sh "$mcp_evidence" management-mcp
cat "$mcp_evidence/result.txt"
```

Ports `127.0.0.1:8080`, `127.0.0.1:8443`, and `54320` must be available. Set
`A3S_CLOUD_BOX_RUN_AS_ROOT=true` when the host requires root-owned namespaces
and cgroups. A local pre-commit rehearsal may set
`A3S_CLOUD_C0_ALLOW_DIRTY=true`; its result is explicitly marked
`dirty-rehearsal` and is not release evidence.

A passing clean default run writes `A3S_CLOUD_C0_1_CROSS_SURFACE_PASS`. A
passing clean MCP run writes `A3S_CLOUD_C0_2M_MANAGEMENT_MCP_PASS`. Both results
bind the exact Cloud, Runtime, and Box revisions and retain only sanitized
scenario evidence, provider logs, and credential-free persistence checks.
The clean MCP scenario also writes the strict
`A3S_CLOUD_W0_2_CERTIFIED ... checks=12/12` result. It reuses the same
production process and PostgreSQL authority to certify W0.2; it does not add a
second Ontology repository, migration runner, Search projection, or test stack.
