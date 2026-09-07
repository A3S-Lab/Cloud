# U0.3 live-host operator path

This note is for operators driving `A3S_CLOUD_U0_3_EXIT_CERTIFIED`. It does
**not** certify exit. Product exit still requires a real enrolled Linux host,
Fleet long-poll, signed Use Registry enrollment, converge digests, collector
evidence, and `run_u0_3_exit_audit.sh`.

## Hard blockers on Darwin control laptops

| Requirement | Darwin status |
| --- | --- |
| Postgres roles + migrate + control-plane API | Runnable (docker `a3s-u03-pg` or compose) |
| `a3s-cloud-node-agent` Fleet long-poll | **Blocked** — Linux only |
| `a3s-box` Runtime provider for the agent | **Blocked** unless installed on the Linux worker |
| `A3S_CLOUD_U0_3_EXIT_CERTIFIED` | **Cannot** be claimed without the Linux host path above |

## Ordered steps

1. Dependency prep (prints migration/serving URLs; never claims EXIT):

```bash
bash tools/use-conformance/run_u0_3_live_host_prep.sh
# If NATS/registry pulls fail:
# A3S_CLOUD_U0_3_PREP_ALLOW_PARTIAL=true bash tools/use-conformance/run_u0_3_live_host_prep.sh
```

2. Start control-plane with the printed URLs:

```bash
export A3S_CLOUD_POSTGRES_MIGRATION_URL='...'
export A3S_CLOUD_POSTGRES_URL='...'
bash tools/dev/run_cloud.sh
```

3. Follow the exact `a3s-cloud` enrollment recipe printed by prep step 3
   (`nodes bootstrap`, Linux install invocation, registry list/catalog inspect,
   `plugin-assignments set`, plan projection confirm). Registry **enrollment**
   uses `a3s-cloud plugin-registries enroll --file=<enroll.json> --idempotency-key=<key>`
   (JSON: name, endpoint, bootstrapRootBase64) against the Plugins REST enroll path.

4. On a Linux worker with `a3s-box`, run the printed agent install invocation,
   then start `a3s-cloud-node-agent` with an absolute `.acl` config derived from
   `config/node.example.acl`.

5. After real plan/apply/observe digests:

```bash
bash tools/use-conformance/collect_live_host_evidence.sh \
  --host HOST --assignment ID --package ID --plan-digest DIGEST
A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION=<cert> \
  bash tools/use-conformance/run_u0_3_exit_audit.sh <evidence-dir>
```

Only a successful audit with operator-owned live-host certification may print
`A3S_CLOUD_U0_3_EXIT_CERTIFIED`.
