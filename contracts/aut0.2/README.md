# AUT0.2 webhook admission contracts

This directory documents the component contract for signed Automation webhook
admission. The Rust values in `crates/contracts/src/automation/webhook.rs`
bind an opaque endpoint to one immutable Automation revision, keep only a
Secret identity and version, bound JSON capture, and record a body-free
delivery decision with replay identity.

`AutomationWebhookEndpointV1` owns the endpoint lifecycle (`active`,
`disabled`, or `revoked`) and a monotonic generation. A request carries a
canonical HMAC-SHA256 signature fact, a bounded base64 capture, the raw-body
digest, and the canonical JSON payload. `AutomationWebhookDeliveryReceiptV1`
binds an admitted or replayed delivery to the same endpoint generation,
revision digest, body digest, and invocation identity. Rejection constructors
make endpoint lifecycle reasons explicit.

Secret material, provider-specific source facts, HTTP listeners, Gateway
routes, schema registries, persistence, and invocation workers remain owned by
their respective runtime boundaries. The endpoint stores only the schema
digest and the Secret identity/version needed for those boundaries to perform
their checks. `crates/control-plane/src/modules/automations` now composes that
contract through an application-owned, atomic in-memory repository: endpoint
creation pins the exact revision, lifecycle writes use generation CAS, and
delivery writes atomically retain the bounded request, receipt, invocation
handoff, redacted audit fact, and admitted Outbox identity. Signature
verification and schema evaluation remain explicit ports, so the component
cannot accidentally claim either check without an infrastructure adapter.
This slice still has no PostgreSQL migration, HTTP listener, Gateway route,
worker, or public webhook availability.
