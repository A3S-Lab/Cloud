# A3S Use Registry conformance

The public-provider gate exercises Cloud's production
`A3sUsePluginRegistryCatalog` through the `PublicInternet` network policy
against the metadata-only signed Registry fixture at the exact A3S Use
dependency revision.

The fixture is owned and signed by A3S Use. Cloud retains only the compatibility
revision and expected bootstrap-root digest; it does not copy TUF signing,
verification, catalog, cursor, cache, or package-download logic. The gate proves
public HTTPS refresh, exact root and role versions, online and cached bounded
reads, root and cached-metadata drift rejection, SSRF and invalid-cursor
rejection, and successful catalog reads when the referenced package target is
absent.

Run it from the Cloud repository root:

```bash
bash tools/use-conformance/run_registry_gate.sh /absolute/evidence/directory
```

The PostgreSQL gate first exercises the production Search adapter through the
same Search-owned constructor selected by the typed process factory and the
non-default `persistence-conformance` owner-port assembly, then exercises the
existing
`PostgresPluginRegistryRepository` and migrations 084-085. It proves
Search tenant/grant isolation and literal-wildcard handling, active-human
authorization is rechecked in the final Registry transaction, concurrent
enrollment replays exactly once, rejected writes leave no Registry, Outbox,
audit, or idempotency residue, reads and the shared Search view stay
tenant-scoped, and non-canonical stored rows fail closed. It exposes no
concrete Search adapter and does not add another Registry store, authorization
evaluator, Outbox, audit log, idempotency implementation, or Search projection.

Run it against an operator-owned PostgreSQL 17 administrative URL:

```bash
A3S_CLOUD_TEST_POSTGRES_URL=postgresql://... \
  bash tools/use-conformance/run_postgres_registry_gate.sh \
  /absolute/evidence/directory
```

When advancing A3S Use, update `Cargo.toml`, `Cargo.lock`, `use-revision`, and
`plugin-v3-root.sha256` together. The script rejects a revision that is not the
exact dependency pin. The upstream fixture is public test data and must never
be enrolled as a deployed Registry trust root.
