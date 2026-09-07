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

The U0.3 assignment PostgreSQL gate exercises
`PostgresPluginAssignmentRepository` and `PostgresPluginPlanProjectionRepository`
through migrations `189`-`191`. It proves concurrent assignment create/replay,
idempotency conflict on changed input, one live `(organization, host, package)`
uniqueness, tenant isolation, FK fail-closed writes with zero residue, plan
projection digest conflict, confirmation CAS, migration ledger presence, and
fail-closed restore of non-canonical workspace scope. It does not exercise Fleet
delivery, Flow steps, or Node Agent apply/observe. The Node Agent authorize-trust
unit slice separately proves request `binding_digest` Artifact binding, opaque
trust-root/policy blob materialize (no directory extract), `a3s-acl` policy
parse, and A3S Use `inspect_bootstrap_root` admission before ack:

```bash
cd crates/node-agent && cargo test --lib authorize_trust
```

The assignment Flow plan-selection slice proves pre-plan observation, package vs
enablement vs already-converged selection, and enablement envelope store. The
disable path is covered by
`enqueue_enablement_plan_selects_disable_and_accepts_planned_ack` (pre-plan
present observation → `PluginHostPlanEnablement` → Planned Disable ack → store →
allow confirmation → digest-only apply → observe InstalledDisabled):

```bash
cd crates/control-plane && cargo test --lib \
  modules::plugins::infrastructure::plugin_assignment_flow
```

Cloud-side crash points 1–5 and 8–9 (assignment before Flow enqueue;
plan/apply/observe command reuse before Fleet ack; confirmation resume from a
stored plan projection) are covered by
`crash_point_1_*` plus Flow unit
`crash_point_2_*`, `crash_point_3_*`, `crash_point_4_5_*`, and
`crash_point_8_9_*`:

```bash
cd crates/control-plane && cargo test --lib crash_point_
```

The U0.3 source-architecture ratchet proves Cloud Plugins does not grow a second
Use platform (no TUF/`a3s-use` embed/capability registry/scheduler) and that
workspace pins stay on `a3s-use-core` + `a3s-use-extension` only:

```bash
cd crates/control-plane && cargo test --lib \
  plugins_u0_assignment_surface_owns_no_second_use_platform
```

The assignment control-surface gate certifies Management MCP catalog pins
(admin mutations vs read-only), TypeScript client assignment/plan paths, CLI
`plugin-assignments` / `plugin-plan-projections`, and the architecture ratchet:

```bash
bash tools/use-conformance/run_assignment_surface_gate.sh /absolute/evidence/directory
```

The U0.3 foundation meta-gate composes recovery, host converge, pinned Use
golden contract fixtures, scope/path/symlink fail-closed, surface parity,
Cloud crash-point suites, the Flow fail-closed/mutation matrix, and Node Agent
Skill/Ui/uninstall/upgrade journal filters (optionally Postgres when
`A3S_CLOUD_TEST_POSTGRES_URL` is set):

```bash
bash tools/use-conformance/run_u0_3_foundation_gate.sh /absolute/evidence/directory
```

CI runs that meta-gate as job `U0.3 assignment foundation` (Use pin checkout,
Bun client/CLI, PostgreSQL 17 assignment gate included). The certification line
includes `mutation_matrix=6` and `node_agent_journal=5` (Skill/Ui/uninstall/
upgrade/enablement on the shared `PluginHostManager` journal).

Pinned Use golden contract fixtures (package identity, catalog digests, plan,
confirmation binding, enablement plan, observation) are certified by:

```bash
bash tools/use-conformance/run_use_contract_fixture_gate.sh /absolute/evidence/directory
```

Managed-scope isolation plus package zip/tar path-escape and symlink rejection
are certified by:

```bash
bash tools/use-conformance/run_use_scope_isolation_gate.sh /absolute/evidence/directory
```

Product-exit audit (foundation-adjacent checks + fail-closed live Cloud↔host
requirement). Without operator certification this exits `2` with
`A3S_CLOUD_U0_3_EXIT_BLOCKED`:

```bash
bash tools/use-conformance/run_u0_3_exit_audit.sh /absolute/evidence/directory
# Optional: A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION=/path/to/cert.txt
```

See `live-host-certification.example.txt` for the required
`A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED` marker line and operator checklist.

Operator live-host evidence collector (writes validated cert + optional
sidecars + checklist; never emits `A3S_CLOUD_U0_3_EXIT_CERTIFIED`):

```bash
bash tools/use-conformance/collect_live_host_evidence.sh \
  --host HOST --assignment ID --package ID --plan-digest DIGEST \
  [--evidence-dir DIR]
# Then:
# A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION=<cert> \
#   bash tools/use-conformance/run_u0_3_exit_audit.sh <evidence>
```

CI fail-closed harness (validator self-check + light exit-audit without live
host; never claims product `A3S_CLOUD_U0_3_EXIT_CERTIFIED`):

```bash
bash tools/use-conformance/run_u0_3_exit_audit_ci.sh
```

Repository-policy CI runs that harness and `bash -n` on the script. Product
exit still requires a real operator live-host certification.

Live-host certification validation (no cargo): the CERTIFIED line must include
`revision=<40hex matching use-revision>` plus nonempty `host`, `assignment`,
`package`, and `plan_digest`, and must reject `PLACEHOLDER_*`. Self-check:

```bash
rev=$(cat tools/use-conformance/use-revision)
# Expect FAIL (PLACEHOLDER_* / bad revision):
bash tools/use-conformance/validate_live_host_certification.sh \
  tools/use-conformance/live-host-certification.example.txt "$rev"; echo exit=$?
# Expect PASS:
tmp=$(mktemp)
printf '%s\n' "A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED revision=$rev host=h1 assignment=a1 package=p1 plan_digest=d1" >"$tmp"
bash tools/use-conformance/validate_live_host_certification.sh "$tmp" "$rev"; echo exit=$?
rm -f "$tmp"
```

Skill-only Node Agent journal converge (shared `PluginHostManager` port, no OKF)
is covered by:

```bash
cd crates/node-agent && cargo test --test plugin_host skill_only
```

UI-only Node Agent journal converge (no Tool/OKF requires; Skill fixture mutated
to a Ui surface) is covered by:

```bash
cd crates/node-agent && cargo test --test plugin_host ui_only
```

Uninstall → Absent/Removed and Upgrade journal (fail-closed lock ownership) on
the same shared `PluginHostManager` journal path:

```bash
cd crates/node-agent && cargo test --test plugin_host uninstall
cd crates/node-agent && cargo test --test plugin_host upgrade
```

Use-side crash points 6–7 (capability publication and old-generation drain) are
certified against the Cloud-pinned A3S Use revision. Prefer the monorepo
`crates/use` checkout at that revision, or set `A3S_USE_CHECKOUT`:

```bash
bash tools/use-conformance/run_use_recovery_gate.sh /absolute/evidence/directory
```

Real-host Skill/Ui converge through `CognitivePackageHostManager` (six-surface
lifecycle including Skill+Ui, plus per-scope lifecycle replay) is certified the
same way:

```bash
bash tools/use-conformance/run_use_host_converge_gate.sh /absolute/evidence/directory
```

Cloud does not embed `a3s-use` while its Flow/Runtime pins diverge from Use
(`a3s-flow` 1.1 vs 1.0, `a3s-runtime` 0.5 vs 0.3). A temporary embed probe
compiles only by dual-versioning those crates; that path is rejected.

Run the PostgreSQL gate against an operator-owned PostgreSQL 17 administrative URL:

```bash
A3S_CLOUD_TEST_POSTGRES_URL=postgresql://... \
  bash tools/use-conformance/run_postgres_assignment_gate.sh \
  /absolute/evidence/directory
```
