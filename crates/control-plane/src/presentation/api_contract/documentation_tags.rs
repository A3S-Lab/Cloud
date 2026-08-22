pub(super) const TAGS: [(&str, &str); 22] = [
    (
        "Platform",
        "Public platform metadata, liveness, and readiness diagnostics.",
    ),
    (
        "Identity",
        "Bootstrap, authentication, credentials, memberships, invitations, and grants.",
    ),
    (
        "Organizations",
        "Organization lifecycle and organization-scoped discovery operations.",
    ),
    (
        "Projects",
        "Project, environment, and immutable attribution lifecycle operations.",
    ),
    (
        "Fleet",
        "Node enrollment, node state, and node-pool placement operations.",
    ),
    (
        "Artifacts",
        "Build-run state, evidence, logs, cancellation, and retry operations.",
    ),
    (
        "Assets",
        "Versioned asset, release, Git transport, and service-profile operations.",
    ),
    (
        "Sources",
        "Source connections, subscriptions, revisions, and signed webhook ingestion.",
    ),
    (
        "Secrets",
        "Secret metadata and write-only secret-version lifecycle operations.",
    ),
    (
        "Edge",
        "Gateway scopes, routes, certificates, claims, policies, and MCP credentials.",
    ),
    (
        "Workloads",
        "Workload publication, deployment, rollback, logs, and binding operations.",
    ),
    (
        "Agents",
        "Agent conversations, executions, events, changes, and cancellation operations.",
    ),
    (
        "Workflow",
        "Ontology, workflow definition, planning, run, task, and template operations.",
    ),
    (
        "Forms",
        "Native form draft, revision, release, and interaction operations.",
    ),
    (
        "Connectors",
        "Environment-scoped Connector profiles and immutable revisions.",
    ),
    (
        "Applications",
        "Application releases, sessions, invocations, messages, and replay.",
    ),
    (
        "Durable Cells",
        "Durable Cell applications, revisions, deployments, routes, and state.",
    ),
    (
        "Operations",
        "Asynchronous operation polling and resumable event streaming.",
    ),
    ("Audit", "Tenant-authorized immutable audit-record queries."),
    (
        "Notifications",
        "Personal notifications, alert policies, and outbound subscriptions.",
    ),
    (
        "Plugins",
        "Plugin Registry discovery and bounded catalog inspection operations.",
    ),
    (
        "Search",
        "Bounded organization-scoped search across authorized resource projections.",
    ),
];
