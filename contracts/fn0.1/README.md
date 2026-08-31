# FN0.1 Function contract

This directory freezes the component-only Function release/profile and
invocation value contracts. It does not claim that Function as a Service is
available.

`cloud.function.profile.v1` belongs with one immutable Assets-owned Function
release and selects exactly one existing lifecycle owner:

| Mode | Exact target | Sole lifecycle owner |
| --- | --- | --- |
| `hosted_task` | OCI artifact plus ExecutionTemplate revision and projection digest | Executions |
| `hosted_service` | OCI artifact plus Workload revision and projection digest | Workloads |
| `external` | Connector profile/revision and definition digest | Connectors |

The profile contains only immutable identity and product intent. Hosted modes
reuse A3S Runtime's closed isolation type. External mode cannot carry local
Runtime isolation or Secret references because its exact Connector revision
owns credentials and egress. Only `hosted_service` may carry optional
protocol/visibility/path/port traffic intent; Edge and Gateway still own the
route and applied snapshot.

The public Rust contract also freezes:

- `cloud.function.invocation.v1`, one tenant/parent/slot/target/input/policy
  authority envelope for direct, Workflow, Agent, and Automation callers;
- digest-bound inline JSON up to 1 MiB or a typed immutable-object reference;
- mode-specific timeout and input/output bounds, a maximum concurrency of
  4,096, an exact authorization digest and egress class, and positive caller
  attempts;
- `cloud.function.invocation-failure.v1`, whose closed failures name the
  selected owner and distinguish terminal, caller-policy, and external
  indeterminate outcomes without scheduling a retry; and
- architecture fitness checks that reject a Function bounded context,
  Function lifecycle tables, mutable provider fields, and non-ACL product
  configuration.

## Frozen fixtures

The hashes cover the exact checked-in bytes, including the final text newline.
Each file parses through `a3s-acl` and regenerates to the exact same canonical
ACL bytes.

| File | Mode | SHA-256 |
| --- | --- | --- |
| [`function-profile-hosted-task.acl`](function-profile-hosted-task.acl) | `hosted_task` | `5264781fcafb5789178adec165d39ec838ca9596ed59e543934956cad9f130cc` |
| [`function-profile-hosted-service.acl`](function-profile-hosted-service.acl) | `hosted_service` | `688fb79d12978ff2fac8c60fd718df4e010e3119957c40298ffe0ec92ffc6f7f` |
| [`function-profile-external.acl`](function-profile-external.acl) | `external` | `0949a9b30c7aacc6c53afac7b45a067ce9379a462cd8659288bfb580c5ac7012` |

Later `FN0.2` through `FN0.6` must bind these values through owner ports and
retain real Runtime/Box, Connector, Workloads, Gateway, recovery, load, and
cross-surface evidence. They may not add another scheduler, queue, process
manager, object client, Secret store, route publisher, retry store, or
autoscaler.
