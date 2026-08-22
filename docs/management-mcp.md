# A3S Cloud Management MCP

## Product boundary

The A3S Cloud management MCP endpoint is an authenticated presentation surface
over existing Cloud application commands and queries. It is not an A0 hosted
MCP asset, a second control plane, a database client, or a path to a node.

The initial `C0.2` slice established the transport, authorization, tenant,
idempotency, and response boundaries with core Project, Environment, and search
tools. The operational-read slice adds Node, Operation, Workload, Deployment,
Route, and BuildRun queries without adding another business or persistence
path. The observability-read slice adds bounded Workload log pages, explicit
BuildRun-log unavailability, and signed BuildRun evidence through the same
application queries and REST response projections. The operational-mutation slice adds five replay-safe
Workload, Deployment, and BuildRun commands through the existing application
handlers and REST response projections.
The backend `W0.2` slice adds seven Ontology create/read/revise/revision/diff
tools over the same Workflow command/query handlers. It does not introduce an
MCP-specific Ontology store, migration policy, ACL parser, or graph database.
The `W0.3` planning slice adds ten Workflow definition, immutable revision,
Goal, and deterministic Plan tools over the same CQRS handlers used by REST,
the maintained client, and CLI. It adds no MCP-owned planner, run engine,
payload store, or authorization path.
The built-in discovery slice adds one read-only Workflow node-catalog tool over
the same project-authorized Workflow query used by REST contract `1.31.0`. It
does not add an MCP-owned catalog, descriptor admission path, provider dispatch,
or Flow mechanism.
The protected HumanTask slice adds two reads and three claim/release/submission mutations
over the same Workflow commands, queries, domain state machine, response DTOs,
repository, idempotency, audit, and Outbox path used by REST, the maintained
client, and CLI. It adds no MCP-owned assignment policy, task store, Form
contract, grant evaluator, or write mechanism.
The native Form lifecycle adds seven draft/release tools over the same Form
commands, queries, owner compiler port, A3S ORM repository, audit, and Outbox
used by REST, the maintained client, and CLI. It adds no MCP-owned Form parser,
compiler, validator, store, or submission path.
The Executions slice adds create/list/exact-get tools for immutable,
project-scoped, ACL-native ExecutionTemplate revisions. They reuse the same
CQRS handlers, `execution:write` scope, Resource Grant checks, idempotency,
A3S ORM repository, audit, Outbox, and REST response DTOs used by contract
`1.24.0`. MCP does not gain a template parser, mutable template store,
scheduler, Runtime provider, or Workflow dispatch path.
The Applications slice adds create/list/get and immutable release
publish/list/get tools over the same project-authorized CQRS, exact Workflow
revision evidence adapter, single PostgreSQL repository, canonical A3S ACL,
idempotency, audit, Outbox, and response DTOs used by REST contract `1.42.0`,
the maintained client, and CLI. MCP owns no Application graph, Flow runtime,
provider, session, Secret, Gateway route, repository, or authorization path.
The `APP0.2-C8` slice adds five `application:write` tools for project-member
session open/read, invocation request/read, and ordered message reads over REST
contract `1.43.0`'s same commands, queries, caller ownership, and response
DTOs. C12 adds three more `application:write` tools for versioned session
close, versioned invocation cancellation, and complete bounded session replay
over REST contract `1.44.0`. MCP owns no end-user credential, answer stream,
second cancellation lifecycle,
Workflow/Flow state, provider, or Gateway delivery path.
The Connector profile slice adds create/revise/profile-list/profile-get and
revision-list/revision-get tools over the same environment-authorized CQRS,
single PostgreSQL repository, canonical A3S ACL parser, response DTOs,
idempotency, Outbox, and audit path used by REST, the maintained client, and
CLI. MCP owns no Connector parser, Secret resolver, profile store, retry rail,
provider client, or execution path, and never exposes resolved endpoint or
credential material.
The Durable Cell slice adds ten application, immutable-revision, deployment,
and route tools over the same `CELL0.4-C2/C3/C4` command/query buses and DTOs
used by REST, the maintained client, and CLI. Its four reads require
`cloud:read`; application/deployment mutations require `workload:write`; route
publication requires `route:write`. Deployment accepts only the same bounded
canonical Service-profile, provider-Workload, and plaintext-free
storage-binding A3S ACL strings and returns references/digests rather than
Secret material. MCP owns no ACL parser, OCI/DNS validator, Cell scheduler,
Workload/Edge controller, S0 lifecycle, repository, or authorization path.
The project-attribution slice adds one current-or-exact immutable read tool and
one optimistic, replay-safe update tool. Both reuse the Projects CQRS,
project-qualified Resource Grant evaluator, A3S ORM repository, shared audit,
and Outbox. MCP owns no attribution store, label validator, billing model, or
migration path.

The personal-notification slice adds recipient-only list, exact get, and
idempotent mark-read tools over the same Notifications CQRS, exact authenticated
Principal, Resource Grant evaluator, A3S ORM repository/migrator, Outbox, and
audit used by REST, the maintained client, and CLI. MCP owns no notification
projection, delivery queue, provider/template/subscription policy, scheduler, or
configuration format.

The personal alert-policy slice adds recipient-only create/list/exact-get/revoke
over the same Notifications CQRS and canonical A3S ACL used by REST, the client,
and CLI. Its two reads require `cloud:read`; create and revoke require
`notification:write`. MCP owns no source registry, expression evaluator,
incident state, projection worker, queue, scheduler, or configuration parser.

## Transport contract

The endpoint is `POST /api/v1/mcp` and implements a sessionless deployment of
MCP protocol version `2026-07-28`. `C0.2m` changes only this presentation
adapter; it retains the verified `C0.2` application, authorization,
idempotency, persistence, and audit boundaries.

- Requests and immediate responses use raw JSON-RPC 2.0 with
  `Content-Type: application/json`.
- Clients advertise both `application/json` and `text/event-stream` in
  `Accept`, even though the stateless first slice returns immediate JSON.
- Every request carries matching protocol version and client-capability
  metadata under `params._meta`, `MCP-Protocol-Version`, and `Mcp-Method`.
  `tools/call`, `prompts/get`, and `resources/read` also carry a matching
  `Mcp-Name`; the Base64 sentinel form is accepted for values that cannot be
  represented safely as an HTTP field value.
- `clientInfo` is optional protocol metadata and is validated when present. It
  is never used as an authenticated identity.
- The endpoint ignores legacy `Mcp-Session-Id` input and never creates or emits
  protocol session state; `GET` and `DELETE /api/v1/mcp` return `405`.
- JSON-RPC batches are rejected. Each HTTP request carries one message.
- JSON-RPC notifications and the legacy `initialize` method are rejected.
- Browser-originated requests are accepted only when `Origin` matches `Host`.
- Transport responses are not wrapped in the REST envelope. A successful or
  failed tool execution carries the same REST success or business-error
  envelope in both `structuredContent` and text content.
- Every successful result is complete and includes `resultType: "complete"`
  plus bounded server metadata. `server/discover` returns the supported
  version, tools capability, private cache scope, and zero discovery TTL.

The endpoint is hidden from the REST OpenAPI document because JSON-RPC and MCP
tool schemas, rather than REST operations, define this transport contract.

## Modern protocol boundary

`C0.2m` removes the legacy initialization state without creating a replacement
session or a second management mechanism. Unsupported versions return JSON-RPC
error `-32022` with the supported and requested versions. Missing or invalid
required body metadata returns HTTP `400` with `-32602`; missing or mismatched
transport headers return HTTP `400` with `-32020`. Unknown methods return HTTP
`404` with JSON-RPC error `-32601`.

The migration does not change Cloud application commands, queries, scopes,
tool catalogs, A3S ORM persistence, or audit behavior. It is also separate
from product gate `MCP0`, which deploys tenant MCP AssetReleases as Runtime
Services behind Gateway.

## Authentication and authorization

The endpoint uses the same bounded API tokens as REST and CLI. Authentication
loads the current token, organization claim, and effective scopes on every
request through the Identity repository. Revocation therefore takes effect on
the next request without an MCP session, cache, Redis, or separate credential
store.

The organization is always derived from the authenticated principal. No tool
accepts an organization identifier, and unknown input properties are rejected
before a command or query runs. Project identifiers remain subject to the same
tenant-aware application queries used by REST.

`cloud:read` is the delegable read-only scope. Organization reads predate
explicit read scopes, so every authenticated tenant token retains the same
baseline reads; `cloud:read` lets a `token:write` issuer create a token with no
mutation capability without granting a new resource read. Existing mutation
scopes control mutation tool visibility and invocation independently:

| Tool | Kind | Required scope |
| --- | --- | --- |
| `a3s_cloud_projects_list` | Query | None |
| `a3s_cloud_project_attribution_get` | Query | None; exact Project Resource Grant enforcement occurs in Projects |
| `a3s_cloud_environments_list` | Query | None |
| `a3s_cloud_applications_list` | Query | `cloud:read`; bounded project-authorized Application heads |
| `a3s_cloud_applications_get` | Query | `cloud:read`; exact Application and current immutable release |
| `a3s_cloud_application_releases_list` | Query | `cloud:read`; bounded immutable release history |
| `a3s_cloud_application_releases_get` | Query | `cloud:read`; one exact immutable release and Workflow evidence |
| `a3s_cloud_applications_create` | Command | `application:write`; Project Resource Grant, canonical release ACL, exact Workflow evidence, and idempotency required |
| `a3s_cloud_application_releases_publish` | Command | `application:write`; Project Resource Grant, positive expected version, canonical release ACL, exact Workflow evidence, and idempotency required |
| `a3s_cloud_application_sessions_open` | Command | `application:write`; exact `project_members` release, caller identity, bounded variables, and idempotency required |
| `a3s_cloud_application_sessions_get` | Query | `application:write`; caller-owned project-member session only |
| `a3s_cloud_application_sessions_close` | Command | `application:write`; caller-owned session, positive expected version, and idempotency required |
| `a3s_cloud_application_sessions_replay` | Query | `application:write`; caller-owned session head, current variables, and bounded contiguous message cursor |
| `a3s_cloud_application_invocations_request` | Command | `application:write`; caller-owned session, exact Ontology/Environment authority, bounded input, and idempotency required |
| `a3s_cloud_application_invocations_get` | Query | `application:write`; caller-owned session and exact invocation only |
| `a3s_cloud_application_invocations_cancel` | Command | `application:write`; caller-owned invocation, positive expected version, Workflow cancellation, and idempotency required |
| `a3s_cloud_application_messages_list` | Query | `application:write`; caller-owned session and bounded exclusive sequence cursor |
| `a3s_cloud_connector_profiles_list` | Query | `cloud:read`; exact Environment Resource Grant enforcement occurs in Connectors |
| `a3s_cloud_connector_profiles_get` | Query | `cloud:read`; returns the profile plus its current immutable revision |
| `a3s_cloud_connector_revisions_list` | Query | `cloud:read`; bounded exact-profile revision history |
| `a3s_cloud_connector_revisions_get` | Query | `cloud:read`; exact immutable revision |
| `a3s_cloud_connector_profiles_create` | Command | `connector:write`; exact Environment Resource Grant, canonical ACL, and idempotency required |
| `a3s_cloud_connector_profiles_revise` | Command | `connector:write`; exact Environment Resource Grant, positive expected version, canonical ACL, and idempotency required |
| `a3s_cloud_forms_list` | Query | None |
| `a3s_cloud_forms_get` | Query | None |
| `a3s_cloud_form_releases_list` | Query | None |
| `a3s_cloud_form_releases_get` | Query | None |
| `a3s_cloud_forms_create` | Command | `form:write` |
| `a3s_cloud_forms_revise` | Command | `form:write` |
| `a3s_cloud_form_releases_publish` | Command | `form:write` |
| `a3s_cloud_memberships_list` | Administrator query | `identity:write` plus organization administrator role |
| `a3s_cloud_memberships_get` | Administrator query | `identity:write` plus organization administrator role |
| `a3s_cloud_memberships_create` | Administrator command | `identity:write` plus organization administrator role; requires explicit `human` or `service` Principal kind |
| `a3s_cloud_memberships_change_role` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_memberships_revoke` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_membership_invitations_list` | Administrator query | `identity:write` plus organization administrator role |
| `a3s_cloud_membership_invitations_get` | Administrator query | `identity:write` plus organization administrator role |
| `a3s_cloud_membership_invitations_create` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_membership_invitations_revoke` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_my_membership_invitations_list` | Principal self-query | `cloud:read`; returns only invitations bound to the authenticated Principal |
| `a3s_cloud_membership_invitations_accept` | Principal self-command | `identity:write`; accepts only an invitation bound to the authenticated Principal |
| `a3s_cloud_resource_grants_list` | Administrator query | `identity:write` plus organization administrator role |
| `a3s_cloud_resource_grants_get` | Administrator query | `identity:write` plus organization administrator role |
| `a3s_cloud_resource_grants_create` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_resource_grants_revoke` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_audit_records_list` | Administrator query | `cloud:read` plus organization administrator role |
| `a3s_cloud_notifications_list` | Principal self-query | `cloud:read`; exact authenticated Principal and Resource Grant filtering apply in Notifications |
| `a3s_cloud_notifications_get` | Principal self-query | `cloud:read`; denied and missing notification IDs share one `404` contract |
| `a3s_cloud_notifications_read` | Principal self-command | `notification:write`; exact Principal, Resource Grant, optimistic concurrency, and idempotency required |
| `a3s_cloud_notification_alert_policies_create` | Principal self-command | `notification:write`; canonical A3S ACL, exact environment scope, Resource Grant, and idempotency required |
| `a3s_cloud_notification_alert_policies_list` | Principal self-query | `cloud:read`; bounded keyset page filtered by current Resource Grants |
| `a3s_cloud_notification_alert_policies_get` | Principal self-query | `cloud:read`; denied and missing policy IDs share one `404` contract |
| `a3s_cloud_notification_alert_policies_revoke` | Principal self-command | `notification:write`; exact Principal, Resource Grant, optimistic concurrency, and idempotency required |
| `a3s_cloud_notification_outbound_subscriptions_create` | Principal self-command | `notification:write`; canonical A3S ACL, exact Connector revision, Resource Grant, and idempotency required |
| `a3s_cloud_notification_outbound_subscriptions_list` | Principal self-query | `cloud:read`; bounded keyset page filtered by current Resource Grants |
| `a3s_cloud_notification_outbound_subscriptions_get` | Principal self-query | `cloud:read`; denied and missing subscription IDs share one `404` contract |
| `a3s_cloud_notification_outbound_subscriptions_revoke` | Principal self-command | `notification:write`; exact Principal, Resource Grant, optimistic concurrency, and idempotency required |
| `a3s_cloud_ontologies_list` | Query | None |
| `a3s_cloud_ontologies_get` | Query | None |
| `a3s_cloud_ontology_revisions_list` | Query | None |
| `a3s_cloud_ontology_revisions_get` | Query | None |
| `a3s_cloud_ontology_revisions_diff` | Query | None |
| `a3s_cloud_ontologies_create` | Command | `ontology:write` |
| `a3s_cloud_ontologies_revise` | Command | `ontology:write` |
| `a3s_cloud_workflow_definitions_list` | Query | None |
| `a3s_cloud_workflow_definitions_get` | Query | None |
| `a3s_cloud_workflow_revisions_list` | Query | None |
| `a3s_cloud_workflow_revisions_get` | Query | None |
| `a3s_cloud_workflow_goals_list` | Query | None |
| `a3s_cloud_workflow_goals_get` | Query | None |
| `a3s_cloud_workflow_plan_revisions_get` | Query | None |
| `a3s_cloud_workflow_node_catalog_get` | Query | None; exact Project Resource Grant enforcement occurs in Workflow |
| `a3s_cloud_human_tasks_list` | Query | None |
| `a3s_cloud_human_tasks_get` | Query | None |
| `a3s_cloud_workflow_definitions_create` | Command | `workflow:write` |
| `a3s_cloud_workflow_definitions_revise` | Command | `workflow:write` |
| `a3s_cloud_workflow_goals_create` | Command | `workflow:write` |
| `a3s_cloud_search` | Query | None |
| `a3s_cloud_plugin_registries_list` | Query | None |
| `a3s_cloud_plugin_registries_get` | Query | None |
| `a3s_cloud_plugin_catalog_search` | Query | None |
| `a3s_cloud_plugin_catalog_search_cached` | Query | None |
| `a3s_cloud_plugin_catalog_inspect` | Query | None |
| `a3s_cloud_plugin_catalog_inspect_cached` | Query | None |
| `a3s_cloud_nodes_list` | Query | None |
| `a3s_cloud_nodes_get` | Query | None |
| `a3s_cloud_operations_list` | Query | None |
| `a3s_cloud_workloads_list` | Query | None |
| `a3s_cloud_workloads_get` | Query | None |
| `a3s_cloud_workload_logs_get` | Query | None |
| `a3s_cloud_deployments_get` | Query | None |
| `a3s_cloud_routes_list` | Query | None |
| `a3s_cloud_routes_get` | Query | None |
| `a3s_cloud_build_runs_list` | Query | None |
| `a3s_cloud_build_runs_get` | Query | None |
| `a3s_cloud_build_run_logs_get` | Query | None |
| `a3s_cloud_build_evidence_get` | Query | None |
| `a3s_cloud_projects_create` | Command | `project:write` |
| `a3s_cloud_project_attribution_update` | Command | `project:write`; Project optimistic concurrency and Resource Grant required |
| `a3s_cloud_environments_create` | Command | `environment:write` |
| `a3s_cloud_workloads_stop` | Command | `workload:write` |
| `a3s_cloud_workloads_rollback` | Command | `workload:write` |
| `a3s_cloud_deployments_cancel` | Command | `workload:write` |
| `a3s_cloud_build_runs_cancel` | Command | `build:write` |
| `a3s_cloud_build_runs_retry` | Command | `build:write` |

A tool that is unavailable to the current principal is absent from
`tools/list` and is indistinguishable from an unknown tool when invoked. The
scope check is repeated during invocation; hiding a tool is never the
authorization boundary.

For a restricted Membership, `a3s_cloud_operations_list` is discoverable only
when the caller has at least one active Resource Grant. That is coarse
admission, not the final decision. `ListOperations` resolves each closed
polymorphic subject kind through its owning Workloads, Artifacts, Executions,
Agents, or Workflow repository, keyset-pages past invisible records, and
returns the same filtered projection used by REST and the Operation snapshot
stream. Unknown, missing, and denied subjects are omitted. The resolver never
reads Operation workflow input as ownership evidence and adds no MCP-local
filter, ownership table, or grant evaluator.

`a3s_cloud_build_run_logs_get` remains discoverable for API compatibility but
returns the standard `503 Service Unavailable` business envelope until Box
exposes an authoritative durable build-log contract. It does not return an
empty success page and does not reuse Workload or Runtime logs.

The six Plugin tools are read-only and reuse the Plugins QueryBus used by REST,
the maintained client, and CLI. Registry list/get return only the Cloud-owned
tenant projection. Catalog search/inspect compose their `host`, `search`, and
release-selector JSON Schemas directly from `a3s-use-extension`; online and
cached tools are distinct and never fall back into each other. They do not
download packages, proxy the local Use management MCP, or introduce a Cloud
catalog/cache model.

## Exact-Principal MembershipInvitation lifecycle

The four administrator tools list immutable invitation history, get one exact
invitation, create an invitation idempotently, and revoke a pending invitation
with optimistic concurrency. Creation names one existing active Principal, one
ordinary Membership role, and an RFC 3339 expiry no more than 30 days ahead.
The two self-service tools ignore the credential's organization claim for
lookup and instead use the exact authenticated Principal across organizations:
`a3s_cloud_my_membership_invitations_list` returns only that Principal's
history, and `a3s_cloud_membership_invitations_accept` returns `404` for a
different Principal just as it does for a missing invitation.

Acceptance reuses the Identity command handler, exact credential Principal,
expected aggregate version, idempotency receipt, A3S ORM transaction, Outbox,
and audit path used by REST, the maintained client, and CLI. It locks the
invitation, creates the ordinary Membership, and records acceptance atomically;
expired, revoked, stale, or duplicate-membership cases cannot leave a partial
Membership. The MCP adapter owns no email lookup, external-identity link,
session, invitation store, RBAC evaluator, notification queue, or scheduler.

## Personal notification inbox

`a3s_cloud_notifications_list` returns only the authenticated Principal's
records and accepts bounded keyset pagination plus an unread-only filter.
`a3s_cloud_notifications_get` resolves one exact recipient record. Both apply
the same current `ResourceAccessEvaluator` as REST, so a resource-scoped record
hidden by revoked or absent grants is omitted or returned as `404`; MCP does not
load a broad inbox and filter it locally.

`a3s_cloud_notifications_read` requires the current aggregate version and a
caller-owned idempotency key. Notifications performs authorization before
replay and atomically persists the unread-to-read transition, one existing
Outbox fact, shared audit, and idempotency receipt. The three tools expose no
recipient selector: recipient identity always comes from the authenticated
credential. They do not project source events or introduce an MCP-specific
store, queue, delivery provider, template/subscription model, scheduler, or
configuration document.

## Personal notification alert policies

REST contract `1.51.0` retains the four
`a3s_cloud_notification_alert_policies_*` tools. Create accepts only one bounded
canonical `cloud.notification.alert-policy.v1` ACL for the closed typed
`edge.domain-claim-status.v1`,
`edge.gateway-certificate-renewal-status.v1`, or
`workload.deployment-health.v1`, or
`edge.gateway-certificate-expiry-status.v1` source and an exact
project/environment scope.
List and exact get apply current Resource Grants; revoke requires the current
aggregate version and a caller-owned idempotency key. Recipient identity always
comes from the authenticated credential.

The response contains the canonical ACL/digest, closed source, exact scope,
recovery preference, lifecycle version, and timestamps. It exposes no arbitrary
event selector, provider failure, metric query, incident state, delivery
attempt, Secret, or credential. The MCP adapter adds no parser, repository,
projector, queue, scheduler, or second event rail.

## Personal outbound notification subscriptions

REST contract `1.46.0` extends the four
`a3s_cloud_notification_outbound_subscriptions_*` tools, which reuse the
same Notifications commands and queries as REST, the maintained client, and
CLI. Create accepts one canonical bounded v1, v2, or
`cloud.notification.outbound-subscription.v3` A3S ACL and binds the
authenticated Principal to an exact Connector revision. v1 retains its fixed
eight-attempt meaning; v2 requires `maximum_provider_attempts` from 1 through
8, while v3 also requires an immutable bounded `suppress_before` event-time
cutoff. List and exact get
apply current Resource Grants; list keyset-pages past invisible records, while
denied and missing exact IDs both return `404`. Revoke uses the current
aggregate version and caller-owned idempotency key.

The response contains the actual definition schema, canonical subscription
ACL/digest, exact Connector identifiers, and immutable
`maximumProviderAttempts` and nullable `suppressBefore`. It never resolves the
Connector endpoint, Secret, credential, provider body, attempt/evidence,
delivery receipt, or retry state.
The MCP adapter adds no recipient selector, repository, configuration parser,
queue, scheduler, retry counter, or delivery mechanism.

## Client flow

Discover the server with an API token. The client sends protocol metadata on
this and every later request:

```bash
curl --request POST "https://cloud.example.com/api/v1/mcp" \
  --header "Authorization: Bearer ${A3S_CLOUD_TOKEN}" \
  --header "Content-Type: application/json" \
  --header "Accept: application/json, text/event-stream" \
  --header "MCP-Protocol-Version: 2026-07-28" \
  --header "Mcp-Method: server/discover" \
  --data '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "server/discover",
    "params": {
      "_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {
          "name": "operator-client",
          "version": "1.0.0"
        }
      }
    }
  }'
```

Then list only the tools visible to that token:

```bash
curl --request POST "https://cloud.example.com/api/v1/mcp" \
  --header "Authorization: Bearer ${A3S_CLOUD_TOKEN}" \
  --header "Content-Type: application/json" \
  --header "Accept: application/json, text/event-stream" \
  --header "MCP-Protocol-Version: 2026-07-28" \
  --header "Mcp-Method: tools/list" \
  --data '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/list",
    "params": {
      "_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
      }
    }
  }'
```

For `tools/call`, also send `Mcp-Name` with the exact tool name. Header and
body metadata are integrity checks, not authorization inputs; the bearer token
continues to determine the tenant and visible tools on every request.

Mutation tools require a caller-owned idempotency key in their arguments. A
REST call and an MCP call with the same command input and key resolve to the
same durable idempotency identity and replay projection.

## Versioned Ontology lifecycle

`a3s_cloud_ontologies_create` accepts `projectId`, at most 1 MiB of closed A3S
ACL, and `idempotencyKey`. `a3s_cloud_ontologies_revise` accepts `ontologyId`,
ACL, a positive `expectedVersion`, `idempotencyKey`, and an optional portable
`migrationRuleId`. The optional rule is not a second policy source: a breaking
deterministic diff is admitted only when that ID resolves to a rule of kind
`migration` in the target ACL. Non-breaking changes derive the `compatible`
policy from the same diff.

The five read tools list/get Ontologies, list/get immutable revisions, and diff
two exact revisions. They return the same REST DTOs and canonical ACL as the
Workflow QueryBus. Create and revise return the same mutation envelope,
including diff and replay status. Historical replay projects the aggregate at
the accepted revision even when a later head exists. All seven tools use the
same PostgreSQL/A3S ORM repository, audit, Outbox, Search projection, tenant
guard, and application handlers as REST and CLI.

## Workflow definition, Goal, and Plan lifecycle

`a3s_cloud_workflow_definitions_create` and
`a3s_cloud_workflow_definitions_revise` accept the canonical closed Workflow
ACL plus the exact typed configuration, data-schema, and policy ACL payloads
referenced by its digests. Their optional revision-semantic envelope contains
three mandatory ACL children for descriptor bindings, the recoverable registry
snapshot, and typed variables, plus optional `variableDefaultsAcl` material that
must exactly cover all digest-backed defaults and optional
`compositeRegionsAcl` material that must exactly cover admitted Iteration and
Loop descriptors. The composite contract freezes bounded region policy and one
exact child WorkflowRevision binding; it does not add an MCP execution path.
Revision additionally requires a positive `expectedVersion`. Both mutations
use `workflow:write`, caller-owned idempotency, immutable revision history,
audit, and Outbox through the same A3S ORM repository as REST.

Definition/revision list and get tools return the aggregate head, immutable
lineage, canonical definition ACL, payload-set digest, and exact canonical
payload ACL. `a3s_cloud_workflow_goals_create` accepts a project ID, bounded
closed Goal ACL, and idempotency key. It binds exact Workflow and Ontology
revision identities/digests and optional Environment identity, then compiles
one immutable Plan revision. Legacy inputs retain
`cloud.workflow.plan.v1`; complete revision-owned semantic contracts compile
`cloud.workflow.plan.v2` with exact descriptor, variable, and semantic-set
digests plus an optional `compositeRegionsDigest`. A graph that selects the
exact finite-Execution descriptor error port compiles Plan v3 and returns each
step's immutable failure contract; execution still uses the same Flow and
Executions authorities. A graph that selects the mutually exclusive exact
default fallback compiles Plan v4 and returns its typed output-port contract;
the existing step `policyDigest` remains the sole exact policy authority.
WorkflowRun v7 folds the same terminal observation into that value and run
queries expose typed projection evidence. Goal list/get and Plan get return the same DTOs as
REST. Identical semantic inputs produce identical canonical Plan bytes and
digest; Goal and Plan identities remain distinct records.

## Built-in Workflow node catalog

`a3s_cloud_workflow_node_catalog_get` requires exactly one `projectId` and is
visible to administrator and read-only catalogs. The handler first resolves the
Project and applies the same Resource Grant evaluator as REST, then returns the
deterministic 23-node projection composed from the frozen parity manifest and
its exact digest-bound node-profile ACL.

The parity manifest remains authoritative for node owner, gate, dependencies,
evidence, and availability. The profile contract adds only coarse kind,
execution class, and semantic profiles. Five nodes are internal, eighteen are
unavailable, none are public, and `parityClaim` is false. Catalog visibility
does not admit descriptor publication or execution; only the exact immutable
registry snapshot owned by a WorkflowRevision can do that. MCP adds no table,
migration, cache, synchronizer, worker, writer, or Flow state.

## Minimal WorkflowRun lifecycle

`a3s_cloud_workflow_runs_start` requires `projectId`, `workflowGoalId`,
`planRevisionId`, and `idempotencyKey`; its optional `timeoutSeconds` is bounded
from 1 through 2,592,000. `a3s_cloud_workflow_runs_cancel` requires one run ID
and idempotency key and accepts an optional bounded reason. Both require
`workflow:write`; cancellation is marked destructive.

The six read-only tools list runs, get one run and its semantic step
projections, wait for at most 30 seconds, return a completed run's bounded
output, page redacted A3S Flow history with a non-negative sequence and a limit
from 1 through 100, and inspect typed variables. All eight tools derive
organization and actor from
the authenticated principal and reuse the REST CQRS handlers, A3S ORM
repository, Operation, A3S Flow history, audit, Outbox, and idempotency
authority. The executor supports Workflow-local `input`, `transform`,
`branch`, `human_decision`, finite `execution`, and `output`. HumanTask
submission is exposed by the protected tool below. Business-service and
remaining provider capability steps, Iteration/Loop execution, and
compensation are not exposed.

`a3s_cloud_workflow_run_variables_get` accepts one `workflowRunId` and returns
the same `cloud.workflow-run.variable-inspection.v1` response as REST contract
`1.33.0`. The shared Workflow query authorizes the owning Project, restores the
exact Plan v2 variable contract, and materializes values from immutable run
input and the correlated A3S Flow history through the execution materializer.
Results preserve declaration order, observed Flow sequence,
materialized/unavailable state, metadata, values, and digests. Secret references
are redacted, pre-Flow immutable inputs may appear at sequence zero, and Plan v1
conflicts. MCP adds no variable table, cache, history, worker, or mutation path.

`a3s_cloud_human_tasks_list` accepts one explicit `projectId`, the closed
optional task status, and an optional limit from 1 through 200. It returns
bounded summaries and never includes an interaction request or large detail
payload. `a3s_cloud_human_tasks_get` resolves one task through Workflow's
existing repository and returns the exact request-bound native A3S Form
interaction only when the authenticated principal is the current claimant.
Both calls use Identity's shared Resource Grant evaluator; an environment-only
grant cannot authorize the project-scoped task, denied and missing IDs share a
`404`, and an unknown assignment-policy revision fails closed.

## Immutable ExecutionTemplate lifecycle

`a3s_cloud_execution_templates_create` requires `projectId`, bounded
`definitionAcl`, and `idempotencyKey`, and requires `execution:write`.
`a3s_cloud_execution_templates_list` requires `projectId` and accepts a limit
from 1 through 200, defaulting to 50. The exact-get tool additionally requires
`templateId` and `revisionId`. The latter two are read-only and visible to a
`cloud:read` principal with project access.

All three derive the organization from authentication, reject unknown
arguments, and reuse the REST command/query handlers and response DTOs. Create
returns `201`, or `200` with `replayed: true` for the exact idempotent replay.
The returned definition is canonical A3S ACL with its semantic digest; MCP
does not compile Workflow input, mutate a revision, or invoke a Runtime Task.

## Immutable Application release lifecycle

`a3s_cloud_applications_create` requires `projectId`, a bounded name, optional
description, canonical `cloud.application.release.v1` `releaseAcl`, and
`idempotencyKey`. `a3s_cloud_application_releases_publish` additionally
requires the exact `applicationId` and a positive `expectedVersion`. Both
require `application:write`, authorize the exact project before replay, match
the named immutable Workflow revision and all six evidence digests, return
`201` for a new release, and return `200` with `replayed: true` for exact
idempotent replay.

The four `cloud:read` tools list bounded Application heads, get one head plus
its current release, list immutable release history, or get one exact release.
Lists accept `limit` from 1 through 200 and default to 50. Every tool derives
the organization from authentication and rejects unknown arguments. Responses
contain canonical release ACL and exact Workflow IDs/digests, but no graph,
payload, Flow history, provider state, session/message state, Secret material,
or Gateway route.

## Project-member Application delivery admission

`a3s_cloud_application_sessions_open` requires one exact Application/release,
optional bounded initial variables, and an idempotency key. It admits only a
`project_members` release, derives the end-user and session identities from the
authenticated Principal and idempotency identity, and returns `201` or an exact
`200` replay. The session read tool returns only that Principal's session.

`a3s_cloud_application_invocations_request` additionally requires one
caller-owned session, exact Ontology/revision, response mode, bounded input,
optional exact Environment, optional bounded timeout, and an idempotency key.
Applications atomically persists the input and immutable execution authority,
then creates or adopts the ordinary deterministic Workflow Goal, Plan, and Run.
Invocation and message reads remain caller-owned; message listing uses an
exclusive sequence cursor and the 100/500 default/maximum bounds.

Contract `1.44.0` adds session close, invocation cancellation, and complete
session replay. Close and cancel require a positive `expectedVersion` and
idempotency key, then delegate to C6's exact optimistic replay and Workflow
cancellation authority. Full replay returns the Applications-owned session
head, current variable revision, bounded contiguous messages, next sequence,
and `hasMore`. All eight delivery tools require `application:write`, including
reads, so a general `cloud:read` token cannot inspect conversation content.

This is management-plane asynchronous admission. `blocking` and `streaming`
are accepted release/invocation intents, but these tools do not wait for or
stream an answer. They add no anonymous or authenticated-end-user application
credential, second cancellation authority, SSE channel, provider execution path, or
Gateway route.

## Immutable Connector profile lifecycle

`a3s_cloud_connector_profiles_create` requires `projectId`, `environmentId`,
a bounded name, canonical `cloud.connector.http.v1` `definitionAcl`, and
`idempotencyKey`. `a3s_cloud_connector_profiles_revise` additionally requires
the exact `profileId` and a positive `expectedVersion`. Both require
`connector:write`, authorize the exact environment before replay, return `201`
for a new revision, and return `200` with `replayed: true` for the exact
idempotent replay.

The four `cloud:read` tools list bounded profile heads, get a profile plus its
current revision, list immutable revision history, or get one exact revision.
Lists accept `limit` from 1 through 200 and default to 50. Every tool derives
the organization from authentication and rejects unknown arguments. Returned
ACL is canonical and digest-linked; resolved Secrets, endpoints, headers,
provider response bodies, and execution-attempt state are never projected.

`a3s_cloud_human_tasks_claim` and `a3s_cloud_human_tasks_release` require one
task ID, a positive `expectedVersion`, and an `idempotencyKey`. They require
`workflow:write`, resolve and authorize the stored project before replay, and
reuse the same Workflow assignment state machine. Claim returns the exact
request-bound Form interaction only to the new claimant; release is accepted
only from that claimant. The repository atomically commits the versioned task,
Outbox event, audit fact, and idempotency record. Neither command is marked
destructive, and both reject unknown arguments and assignment-policy revisions.

For a restricted Membership, Ontology and Workflow create/list/start tools
authorize their explicit `projectId` before dispatch. Indirect Ontology,
definition, Goal, Run, and HumanTask detail tools receive only coarse project-family admission
from the MCP catalog; the shared Workflow application resolver loads the
owning aggregate and authorizes its canonical project before revision, Plan,
wait, output, history, task detail, revise, cancel, or idempotency replay.
Revisions and plans inherit their parent scope, while HumanTask uses its stored
canonical project. An environment-only grant cannot authorize a project
aggregate, denied and missing IDs share one `404`, and revocation takes effect
on the next stateless MCP request.

## Native Form draft and release lifecycle

`a3s_cloud_forms_create` accepts `projectId`, a bounded name and optional
description, one native Form `document` JSON object, and `idempotencyKey`.
`a3s_cloud_forms_revise` replaces `projectId` with `formId` and adds a positive
`expectedVersion`. Both reject unknown properties, non-object documents,
invalid UUIDs, zero versions, and unsafe idempotency keys before dispatch.

Draft list/get and release list/get tools return the same REST DTOs. Release
publication accepts `formId`, `expectedVersion`, and `idempotencyKey`, then
calls the pinned A3S Form owner compiler through the Forms application port.
It atomically commits the next aggregate version, immutable release,
idempotency record, audit, and Outbox through A3S ORM. Exact create, revise, and
publish replay returns the historical accepted projection even after the
aggregate advances. The adapter derives organization and actor identity from
the principal and never accepts either as an argument. It does not compile or
validate Form semantics itself and does not expose Form submission or
HumanTask submission.

For a restricted Membership, create/list use their explicit `projectId` while
get, revise, publish, and release tools receive only coarse project-family
admission from the MCP catalog. The shared Forms application resolver loads the
draft's canonical project and makes the final Resource Grant decision before
reads or idempotency replay. Denied and missing Form IDs therefore return the
same `404`, revocation applies on the next request, and an environment-only
grant cannot authorize a project-scoped Form.

## Bounded observability reads

`a3s_cloud_workload_logs_get` accepts `workloadId`, `revisionId`, and optional
`cursor`, `limit`, and `stream` arguments. Cursors use the opaque
REST-compatible `v1:<sequence>` form, the default page contains 100 records,
the maximum is 256, and `stream` is either `stdout` or `stderr`. Responses reuse
the REST log DTOs and retain explicit gap records and the next opaque cursor.

`a3s_cloud_build_run_logs_get` accepts `buildRunId` and the same bounded
arguments so its public contract remains stable, but currently returns the
standard `503 Service Unavailable` envelope. Box has not yet exposed the
authoritative durable build-log records needed for a successful page.

`a3s_cloud_build_evidence_get` accepts only `buildRunId` and returns the same
signed SPDX/SLSA evidence projection as REST. The MCP surface performs no live
node read and exposes no SSE stream: retained log objects, metadata, and
evidence continue through the existing QueryBus handlers, A3S ORM repositories,
and configured object store. Existing ingestion redaction and tenant guards
remain authoritative.

## Replay-safe operational mutations

`a3s_cloud_workloads_stop`, `a3s_cloud_deployments_cancel`, and
`a3s_cloud_build_runs_cancel` are marked destructive. Workload rollback and
BuildRun retry are non-destructive recovery actions. All five are non-read-only,
idempotent, closed-world tools.

Every invocation requires `idempotencyKey` with 1 through 255 header-safe UTF-8
bytes. Workload stop also requires `workloadId`; rollback requires `workloadId`
and `sourceRevisionId`; Deployment cancel requires `deploymentId`; and both
BuildRun commands require `buildRunId`. Unknown properties, missing fields,
invalid UUIDs, empty or oversized keys, and newline-bearing keys fail as
JSON-RPC invalid parameters before command dispatch.

The adapter derives the organization from the authenticated principal, adds
the request ID and current timestamp where the existing command requires them,
and dispatches through `CommandBus`. It does not read a repository, SQL, Redis,
an object store, or a node. Command status mirrors the reused application
handler (`201` for created resources, `202` for accepted operations, or `200`
for synchronous transitions); exact replays return `200` and `replayed: true`
where the underlying contract is replayable.

## Conformance

The dedicated `C0.2m` scenario in
[`tools/c0-conformance`](../tools/c0-conformance/README.md) boots the production
control-plane binary with the shipped A3S ACL configuration and digest-pinned
PostgreSQL 17. It first proves `server/discover`, per-request version and
client metadata, exact transport-header matching, legacy initialization
removal, and unsupported-version errors. The verified pre-extension evidence
proved the exact 23-tool administrator and 16-tool `cloud:read` catalogs. The
current focused source runner requires exact 129-tool administrator and 70-tool
`cloud:read` catalogs and their read-only, destructive, idempotent, and
closed-world annotations; denies a hidden mutation without a database write;
replays one REST Project command through MCP using the same durable idempotency
record; returns the same `404` business-error contract for foreign and missing
Projects; and queries the shared tenant audit history with the read-only
administrator token while proving the response omits internal `details`. It
also creates an Ontology through REST, replays it through
MCP, exercises all seven Ontology tools with a read-only token where
applicable, rejects a breaking revision without its target migration rule,
publishes the explicit migration, and proves historical replay after later
revisions. It creates a native Form through REST, replays creation through
MCP, revises and publishes through MCP, exercises all four read tools with the
read-only token, and proves publication plus historical revision replay. It
creates a real Environment, exercises all five operational
list tools, verifies all eight detail/log/evidence tools and all five commands
return bounded `NOT_FOUND` envelopes for missing resources, and rejects invalid
read and command arguments. It also creates one Workload from A3S ACL, stops it
through MCP, proves exact replay, and observes token revocation on the next MCP
request. The persistence check requires the expected Token digests, read-only
scope, revocation, Project, Ontology, Form draft/release, Environment, stopped
Workload, and idempotency rows, plus zero plaintext credentials in responses,
logs, evidence, or the PostgreSQL dump. Production persistence reaches
PostgreSQL only through A3S ORM repositories.

The expanded focused catalog, permission, Ontology migration, Workflow
definition/Goal/Plan lifecycle, built-in node-catalog cross-surface equality,
native Form lifecycle, minimal WorkflowRun,
protected HumanTask read/claim/release/privacy, tenant/role boundary, deterministic-plan,
immutable Application release lifecycle, immutable Connector profile/revision lifecycle, Durable Cell application and
deployment lifecycle with Secret-free responses, strict-boundary, and replay
tests pass. The updated clean PostgreSQL/A3S Box
scenario and its Ontology, Workflow, Form, and WorkflowRun
persistence/idempotency assertions must pass before these slices are verified.
The expanded provider scenario publishes the shared
`contracts/w0.3/execution-template.acl` through REST, proves exact replay and
read-only list/get through MCP, rejects an unknown ACL field without consuming
idempotency, makes foreign and missing Projects indistinguishable, and asserts
the exact revision, Outbox, audit, migration `098`, and immutability-trigger
rows in PostgreSQL. Its clean Linux run plus the separate seven-boundary
WorkflowRun/finite-child `SIGKILL` run remain required before the finite
Workflow step is called verified.

## Current limits

`C0.2m` is verified by the clean Linux PostgreSQL/A3S Box conformance gate and
retains bearer-token authentication. OAuth 2.1 discovery and consent follow
only after the token-scoped confused-deputy gate.
Secret material, exec, terminal access, server-side sessions, live log
streams, and JSON-RPC batching are not exposed by this slice. No additional
mutation is admitted without its existing scope, idempotency contract, tenant
boundary, and audit behavior.

## Protocol references

- [MCP versioning and compatibility, revision 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [MCP Streamable HTTP, revision 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- [MCP server discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
