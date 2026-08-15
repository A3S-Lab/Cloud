# CELL0.1 Durable Cell contracts

This directory freezes the ACL-native producer fixtures for the Durable Cell
domain foundation. It is not provider certification or a service-availability
claim.

| Fixture | Schema | Canonical ACL digest |
| --- | --- | --- |
| [`service-profile.acl`](service-profile.acl) | `cloud.durable-cell.service.v1` | `sha256:55422ee8bc0028a10e09aef7487e321511cbcc05545d693338b5cc086d43b303` |
| [`application.acl`](application.acl) | `cloud.durable-cell.application.v1` | `sha256:5c4047cc251bfde4f2c3ce2677347fdce91fe7199ecd4477e16ce21513c2ea87` |

Both fixtures are parsed, regenerated, profile-bound, and digest-checked by the
Durable Cells owner implementation through `a3s-acl`. The application fixture
binds one existing BuildRun, one immutable bundle, the exact Service-profile
digest, two ordered Cell classes, and their state read/write versions.

## Projection identity

`DurableCellProjectionIdentity` freezes only cross-context correlation:

| Lifetime | Existing or reserved identity |
| --- | --- |
| Stable for the application | `StorageNamespaceId`, `WorkloadId` |
| Stable for one application revision | `WorkloadRevisionId`, Workloads `DeploymentId`, `OperationId` |
| Selected by environment orchestration later | Existing `GatewayScopeId` |

The Workload uses the existing managed-owner kind
`durable-cell.application`, with the application ID, application revision
number, and application-definition digest as its fence. There is no separate
Durable Cell deployment state machine or deployment ID. Workloads owns rollout,
Operations owns long-running progress, S0 will own namespace lifecycle, and
Gateway owns applied routing state.
