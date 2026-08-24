# P0.2 workload profile contract

`workload-profile.acl` freezes the component-only `P0.2-C1/C2` desired-state
contract. Canonical `a3s.cloud.workload-profile.v1` binds one explicit `web`,
`worker`, or `scheduled_task` profile to an exact accepted BuildPlan, Source
revision, and project root. Process, resources, Secrets references, Service
ports, health, public-route intent, and scheduled Task policy are closed and
parsed/generated only through `a3s-acl`.

`P0.2-C1` compiles web and worker profiles into existing Workloads-owned
`ServiceTemplate` values and scheduled profiles into existing
Executions-owned `ExecutionTemplate` values after verifying one successful
exact BuildRun and BuildEvidence. Compilation does not write those owner
records or evaluate schedules.

`P0.2-C2` adds authorization-first acceptance and migration `147`. Stable
logical profile IDs and continuous revision numbers preserve append-only
history across BuildPlans. A same-actor submission of the exact current
contract converges without another revision; another actor's decision or a
changed contract creates the next immutable revision. PostgreSQL atomically
stores the revision, idempotency reference, audit record, and Outbox event and
reparses canonical ACL on reads.

This slice creates no BuildRun, Workload, Route, Execution, Automation, timer,
or scheduler record. Production composition, public interfaces, and handoff to
those owning contexts remain later P0 work.
