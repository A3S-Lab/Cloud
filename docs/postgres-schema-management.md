# PostgreSQL schema management

## Authority

Cloud owns the ordered SQL manifest in [`migrations/`](../migrations). A3S ORM
is the sole migration executor: it validates the manifest, serializes
concurrent runners with a PostgreSQL advisory transaction lock, applies each
pending migration transactionally, and records the version and checksum in
`a3s_orm_migrations`.

`a3s-cloud-migrate` is the only production process root that invokes that
executor. It is a one-shot process, not a `server.role`; it registers no HTTP
route, background worker, event transport, repository family, or provider
client. API, Worker, Relay, and `all` call only `connect_postgres`, which reads
the migration ledger and cannot create or alter schema objects.

This separation prevents a serving replica from becoming an implicit
installer and keeps migration execution in one existing mechanism rather than
adding a Cloud lock, migration table, or retry loop.

## Run order

With the closed Cloud ACL and PostgreSQL credential available through its
named environment variable, run:

```bash
cargo run -p a3s-cloud-control-plane --bin a3s-cloud-migrate -- config/cloud.acl
cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

The repository development launcher performs the same order. A production
installation or upgrade must preserve this sequence:

1. make PostgreSQL reachable and prove backup/restore readiness;
2. stop before migration if the release notes require an incompatible
   contract phase;
3. run the exact target release's `a3s-cloud-migrate` job to successful exit;
4. start target API, Worker, and Relay replicas;
5. retain old replicas only while every applied change is expand-compatible;
6. drain old replicas before any later contract migration removes their
   admitted schema.

Re-running the exact migrator is safe. Concurrent jobs converge through the
A3S ORM advisory lock: one applies pending versions and the other reports that
the schema is already current.

## Serving-process admission

A serving build admits a database only when every migration in that build's
manifest exists with the exact recorded checksum. It intentionally accepts
additional later migration records so an old and new binary can overlap during
an expand-compatible rolling upgrade.

Admission fails before domain repositories or workers are constructed when:

- the migration ledger does not exist;
- a required version is missing; or
- a required checksum differs from the compiled Cloud manifest.

Readiness repeats the same check. It never applies a pending migration and
never treats the existence of one arbitrary migration row as schema readiness.
An unknown later version is not permission to perform a breaking contract
change; compatibility remains an installer/release obligation.

## Remaining production boundary

The executable boundary and PostgreSQL 17 concurrency/admission gate are
implemented. `H0.4` remains open until the ACL-native Box installation gives
the migration job and serving roles distinct least-privilege PostgreSQL
principals, packages their exact upgrade/rollback order, and retains clean
Linux, failover, backup, restore, and contract-migration evidence. Those
installation duties must use the same A3S ORM migration ledger; they must not
introduce another migration runner or mutable schema version authority.
