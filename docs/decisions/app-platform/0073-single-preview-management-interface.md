# 0073: Expose one pull-request Preview management interface

Status: Accepted

## Context

`P0.3-C1` through `C6` already define the canonical pull-request Preview
Policy, append-only policy revisions, durable `PullRequestPreview` projection,
authorization-first acceptance, and exact owner handoffs. Those authorities
were internal. Operators and maintained automation could neither inspect the
accepted policy lineage nor observe the current behavioral state of one pull
request.

Implementing REST, Management MCP, client, and CLI independently would create
competing authorization, page bounds, restored-state validation, response
projections, or repository reads. It could also expose Sources-private webhook
evidence or downstream owner state and incorrectly turn a read surface into a
second Preview lifecycle.

The policy revision and pull-request Preview are different domain resources.
They share scope and authorization mechanics, but they do not share one
aggregate or repository. Combining their read authority would erase that
boundary; duplicating it in Presentation would bypass it.

## Decision

`P0.3-C7` adds two narrow Developer Workflows Application services:

- `PreviewPolicyQueryService` owns current, exact-revision, and bounded
  revision-history reads through `IPullRequestPreviewPolicyRepository`; and
- `PullRequestPreviewQueryService` owns one exact current behavioral Preview
  read through `IPullRequestPreviewProjectionRepository`.

Both depend on the existing `IDeveloperWorkflowAuthorizationPort` and run the
exact Environment authorization before validating private subscription,
revision, or pull-request identifiers. The policy service reparses and
validates restored canonical ACL, exact tenant/Project/Environment/subscription
scope, a `1..=100` page, and a continuous ascending sequence beginning at one.
The Preview service revalidates the restored aggregate, immutable policy
authority, exact scope, and one portable positive pull-request identity no
larger than `9,007,199,254,740,991`.

The production root constructs both services once, registers their four typed
queries once on the existing CQRS bus, and shares the same authorization port
instance used by BuildPlan, WorkloadProfile, and Preview Policy acceptance.
No public adapter receives either repository interface.

The public projection over those existing authorities is:

- REST accepts one canonical `policyAcl`, gets the current revision, lists
  bounded revision history, gets one exact revision, and gets one exact
  pull-request Preview under the existing Organization/Project/Environment
  path;
- OpenAPI contract `1.75.0` publishes the same closed requests, bounds,
  statuses, and fully typed responses;
- the maintained TypeScript client validates only transport bounds and the CLI
  requires a `.acl` policy file before calling those REST operations; and
- five Management MCP tools dispatch the same command and four queries through
  the shared buses and reuse the REST response DTOs.

The maintained surface mapping is exact:

| Capability | REST suffix under one Environment | TypeScript client | CLI | Management MCP | Scope |
| --- | --- | --- | --- | --- | --- |
| Accept policy | `POST /pull-request-preview-policies` | `acceptPullRequestPreviewPolicy` | `preview-policies accept` | `a3s_cloud_pull_request_preview_policies_accept` | `build:write` |
| Get current policy | `GET /pull-request-preview-policies/{sourceSubscriptionId}` | `getCurrentAcceptedPullRequestPreviewPolicyRevision` | `preview-policies get` | `a3s_cloud_pull_request_preview_policies_get` | `cloud:read` |
| List policy history | `GET /pull-request-preview-policies/{sourceSubscriptionId}/revisions` | `listAcceptedPullRequestPreviewPolicyRevisions` | `preview-policy-revisions list` | `a3s_cloud_pull_request_preview_policy_revisions_list` | `cloud:read` |
| Get exact policy revision | `GET /pull-request-preview-policies/{sourceSubscriptionId}/revisions/{revisionId}` | `getAcceptedPullRequestPreviewPolicyRevision` | `preview-policy-revisions get` | `a3s_cloud_pull_request_preview_policy_revisions_get` | `cloud:read` |
| Get current Preview | `GET /pull-request-previews/{sourceSubscriptionId}/pull-requests/{pullRequestId}` | `getPullRequestPreview` | `pull-request-previews get` | `a3s_cloud_pull_request_previews_get` | `cloud:read` |

Acceptance requires coarse `build:write`; reads require coarse `cloud:read`.
Those scopes are transport admission only. Exact active Membership, Resource
Grant, Project, and Environment policy remains behind the Application
authorization port on every surface. Management MCP binds all five tools to
explicit Project and Environment arguments.

Requests are closed and accept no caller-authored policy object, webhook body,
delivery identity, source credential, projection receipt, or lifecycle state.
Responses contain canonical policy ACL and digest, immutable revision facts,
behavioral repository/branch/quota/trust/expiry/status data, and stable owner
references. They contain no Secret material, provider delivery evidence,
checkout state, SourceRevision/BuildRun/Workload/Execution/Route/Operation
state, or cleanup command. A3S ACL remains the only product configuration
language and is parsed only by the existing Domain contract through
`a3s-acl`.

C7 adds no schema, table, migration, aggregate, parser, repository,
authorization evaluator, Inbox, Outbox, Relay, queue, worker, retry rail,
provider client, timer, scheduler, or owner lifecycle handoff.

## Consequences

- REST, Management MCP, client, and CLI share one authorization, restored-state
  validation, revision-order, page-bound, integer-bound, and response authority.
- Policy history and current Preview state remain separate resources with
  interface-sized Application services rather than a new cross-aggregate
  manager.
- Invalid, discontinuous, oversized, or cross-scope repository state fails
  closed before Presentation can serialize it.
- Public observation does not imply that pre-acceptance source discovery,
  Workload/Execution/Route/Operation scheduling, expiry execution, or cleanup
  is available; those remain owner-specific P0.3 work.
