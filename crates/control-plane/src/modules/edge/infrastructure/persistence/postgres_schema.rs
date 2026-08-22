use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct Nodes => "nodes" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
    }
}

orm_table! {
    pub(super) struct Routes => "routes" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        gateway_node_id: Uuid => "gateway_node_id",
        hostname: String => "hostname",
        path_prefix: String => "path_prefix",
        workload_id: Uuid => "workload_id",
        workload_revision_id: Uuid => "workload_revision_id",
        runtime_unit_id: String => "runtime_unit_id",
        runtime_generation: u64 => "runtime_generation",
        port_name: String => "port_name",
        upstream_origin: String => "upstream_origin",
        target_observed_at: DateTime<Utc> => "target_observed_at",
        state: String => "state",
        gateway_revision: u64 => "gateway_revision",
        gateway_command_id: Uuid => "gateway_command_id",
        snapshot_digest: String => "snapshot_digest",
        failure: Option<String> => "failure",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        activated_at: Option<DateTime<Utc>> => "activated_at",
        domain_claim_id: Option<Uuid> => "domain_claim_id",
        domain_pattern: Option<String> => "domain_pattern",
        gateway_certificate_id: Option<Uuid> => "gateway_certificate_id",
    }
}

orm_table! {
    pub(super) struct DomainClaims => "domain_claims" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        pattern: String => "pattern",
        challenge_dns_name: String => "challenge_dns_name",
        challenge_value: String => "challenge_value",
        state: String => "state",
        failure: Option<String> => "failure",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        verified_at: Option<DateTime<Utc>> => "verified_at",
        revoked_at: Option<DateTime<Utc>> => "revoked_at",
    }
}

orm_table! {
    pub(super) struct GatewayCertificates => "gateway_certificates" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        node_id: Uuid => "node_id",
        domain_claim_ids: serde_json::Value => "domain_claim_ids",
        gateway_revision: u64 => "gateway_revision",
        gateway_command_id: Uuid => "gateway_command_id",
        snapshot_digest: String => "snapshot_digest",
        request: serde_json::Value => "request",
        state: String => "state",
        csr_digest: Option<String> => "csr_digest",
        serial_number: Option<String> => "serial_number",
        fingerprint: Option<String> => "fingerprint",
        certificate_pem: Option<String> => "certificate_pem",
        ca_bundle_pem: Option<String> => "ca_bundle_pem",
        issued_at: Option<DateTime<Utc>> => "issued_at",
        expires_at: Option<DateTime<Utc>> => "expires_at",
        failure: Option<String> => "failure",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        ready_at: Option<DateTime<Utc>> => "ready_at",
        revoked_at: Option<DateTime<Utc>> => "revoked_at",
    }
}

orm_table! {
    pub(super) struct GatewayRouteCutovers => "gateway_route_cutovers" {
        deployment_id: Uuid => "deployment_id",
        organization_id: Uuid => "organization_id",
        workload_id: Uuid => "workload_id",
        previous_revision_id: Uuid => "previous_revision_id",
        candidate_revision_id: Uuid => "candidate_revision_id",
        previous_generation: u64 => "previous_generation",
        candidate_generation: u64 => "candidate_generation",
        node_id: Uuid => "node_id",
        gateway_revision: u64 => "gateway_revision",
        gateway_command_id: Uuid => "gateway_command_id",
        gateway_certificate_id: Uuid => "gateway_certificate_id",
        snapshot_digest: String => "snapshot_digest",
        snapshot_expires_at: DateTime<Utc> => "snapshot_expires_at",
        routes: serde_json::Value => "routes",
        state: String => "state",
        failure: Option<String> => "failure",
        staged_at: DateTime<Utc> => "staged_at",
        acknowledged_at: Option<DateTime<Utc>> => "acknowledged_at",
    }
}

orm_table! {
    pub(super) struct GatewayCertificateConvergences => "gateway_certificate_convergences" {
        organization_id: Uuid => "organization_id",
        node_id: Uuid => "node_id",
        gateway_revision: u64 => "gateway_revision",
        gateway_command_id: Uuid => "gateway_command_id",
        previous_certificate_id: Uuid => "previous_certificate_id",
        replacement_certificate_id: Option<Uuid> => "replacement_certificate_id",
        snapshot_digest: String => "snapshot_digest",
        retained_routes: serde_json::Value => "retained_routes",
        rejected_routes: serde_json::Value => "rejected_routes",
        reason: String => "reason",
        state: String => "state",
        failure: Option<String> => "failure",
        staged_at: DateTime<Utc> => "staged_at",
        acknowledged_at: Option<DateTime<Utc>> => "acknowledged_at",
    }
}

orm_table! {
    pub(super) struct GatewayCertificateExpiryRisks => "gateway_certificate_expiry_risks" {
        organization_id: Uuid => "organization_id",
        route_id: Uuid => "route_id",
        node_id: Uuid => "node_id",
        state: String => "state",
        active_certificate_id: Uuid => "active_certificate_id",
        active_certificate_expires_at: DateTime<Utc> => "active_certificate_expires_at",
        gateway_revision: u64 => "gateway_revision",
        generation: u64 => "generation",
        previous_at_risk_certificate_id: Option<Uuid> => "previous_at_risk_certificate_id",
        previous_at_risk_certificate_expires_at: Option<DateTime<Utc>> => "previous_at_risk_certificate_expires_at",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct GatewayRouteScopes => "gateway_route_scopes" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        node_id: Uuid => "node_id",
        membership_generation: u64 => "membership_generation",
        min_ready: u32 => "min_ready",
        max_unavailable: u32 => "max_unavailable",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct GatewayScopeMembers => "gateway_scope_members" {
        gateway_scope_id: Uuid => "gateway_scope_id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        node_id: Uuid => "node_id",
        ordinal: u32 => "ordinal",
        membership_generation: u64 => "membership_generation",
        added_at: DateTime<Utc> => "added_at",
    }
}

orm_table! {
    pub(super) struct GatewayRollouts => "gateway_rollouts" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        membership_generation: u64 => "membership_generation",
        generation: u64 => "generation",
        correlation_id: Uuid => "correlation_id",
        min_ready: u32 => "min_ready",
        max_unavailable: u32 => "max_unavailable",
        desired_replicas: u32 => "desired_replicas",
        state: String => "state",
        ready_replicas: u32 => "ready_replicas",
        unavailable_replicas: u32 => "unavailable_replicas",
        aggregate_version: u64 => "aggregate_version",
        started_at: DateTime<Utc> => "started_at",
        completed_at: Option<DateTime<Utc>> => "completed_at",
    }
}

orm_table! {
    pub(super) struct GatewayRolloutReplicas => "gateway_rollout_replicas" {
        gateway_rollout_id: Uuid => "gateway_rollout_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        membership_generation: u64 => "membership_generation",
        node_id: Uuid => "node_id",
        revision: u64 => "revision",
        command_id: Uuid => "command_id",
        snapshot_digest: String => "snapshot_digest",
        snapshot_expires_at: DateTime<Utc> => "snapshot_expires_at",
        gateway_certificate_id: Option<Uuid> => "gateway_certificate_id",
        state: String => "state",
        failure: Option<String> => "failure",
        acknowledged_at: Option<DateTime<Utc>> => "acknowledged_at",
        recovery: Option<serde_json::Value> => "recovery",
    }
}

orm_table! {
    pub(super) struct GatewayRolloutRollbacks => "gateway_rollout_rollbacks" {
        failed_rollout_id: Uuid => "failed_rollout_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        membership_generation: u64 => "membership_generation",
        failed_generation: u64 => "failed_generation",
        rollback_rollout_id: Uuid => "rollback_rollout_id",
        rollback_generation: u64 => "rollback_generation",
        state: String => "state",
        aggregate_version: u64 => "aggregate_version",
        required_at: DateTime<Utc> => "required_at",
        staged_at: Option<DateTime<Utc>> => "staged_at",
        completed_at: Option<DateTime<Utc>> => "completed_at",
        failure: Option<String> => "failure",
    }
}

orm_table! {
    pub(super) struct GatewayRouteProjections => "gateway_route_projections" {
        gateway_rollout_id: Uuid => "gateway_rollout_id",
        route_id: Uuid => "route_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        membership_generation: u64 => "membership_generation",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        gateway_node_id: Uuid => "gateway_node_id",
        hostname: String => "hostname",
        path_prefix: String => "path_prefix",
        workload_id: Uuid => "workload_id",
        workload_revision_id: Uuid => "workload_revision_id",
        runtime_unit_id: String => "runtime_unit_id",
        runtime_generation: u64 => "runtime_generation",
        port_name: String => "port_name",
        upstream_origin: String => "upstream_origin",
        target_observed_at: DateTime<Utc> => "target_observed_at",
        state: String => "state",
        gateway_revision: u64 => "gateway_revision",
        gateway_command_id: Uuid => "gateway_command_id",
        snapshot_digest: String => "snapshot_digest",
        failure: Option<String> => "failure",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        activated_at: Option<DateTime<Utc>> => "activated_at",
        domain_claim_id: Option<Uuid> => "domain_claim_id",
        domain_pattern: Option<String> => "domain_pattern",
        gateway_certificate_id: Option<Uuid> => "gateway_certificate_id",
    }
}

orm_table! {
    pub(super) struct GatewayRouteOwnership => "gateway_route_ownership" {
        gateway_rollout_id: Uuid => "gateway_rollout_id",
        route_id: Uuid => "route_id",
        gateway_node_id: Uuid => "gateway_node_id",
        hostname: String => "hostname",
        path_prefix: String => "path_prefix",
        created_at: DateTime<Utc> => "created_at",
    }
}

orm_table! {
    pub(super) struct GatewayScopes => "gateway_scopes" {
        node_id: Uuid => "node_id",
        last_issued_revision: u64 => "last_issued_revision",
        installed_revision: Option<u64> => "installed_revision",
        aggregate_version: u64 => "aggregate_version",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct GatewayPublications => "gateway_publications" {
        node_id: Uuid => "node_id",
        revision: u64 => "revision",
        expected_revision: Option<u64> => "expected_revision",
        command_id: Uuid => "command_id",
        command_correlation_id: Uuid => "command_correlation_id",
        snapshot_digest: String => "snapshot_digest",
        acl: String => "acl",
        state: String => "state",
        failure: Option<String> => "failure",
        command_issued_at: DateTime<Utc> => "command_issued_at",
        command_not_after: DateTime<Utc> => "command_not_after",
        snapshot_expires_at: DateTime<Utc> => "snapshot_expires_at",
        acknowledged_at: Option<DateTime<Utc>> => "acknowledged_at",
        certificate_request: Option<serde_json::Value> => "certificate_request",
    }
}

orm_table! {
    pub(super) struct McpGatewaySnapshotPublications => "mcp_gateway_snapshot_publications" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        node_id: Uuid => "node_id",
        gateway_revision: u64 => "gateway_revision",
        gateway_command_id: Uuid => "gateway_command_id",
        snapshot_digest: String => "snapshot_digest",
        desired_state_digest: String => "desired_state_digest",
        mcp_route_count: u32 => "mcp_route_count",
        publication_owner: String => "publication_owner",
        staged_at: DateTime<Utc> => "staged_at",
    }
}

orm_table! {
    pub(super) struct McpServiceProfiles => "mcp_service_profiles" {
        organization_id: Uuid => "organization_id",
        asset_id: Uuid => "asset_id",
        asset_release_id: Uuid => "asset_release_id",
        profile_digest: String => "profile_digest",
        acl: String => "acl",
        created_at: DateTime<Utc> => "created_at",
    }
}

orm_table! {
    pub(super) struct McpRoutePolicies => "mcp_route_policies" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        domain_claim_id: Uuid => "domain_claim_id",
        workload_id: Uuid => "workload_id",
        asset_id: Uuid => "asset_id",
        asset_release_id: Uuid => "asset_release_id",
        profile_digest: String => "profile_digest",
        hostname: String => "hostname",
        path: String => "path",
        policy_revision: u64 => "policy_revision",
        policy_digest: String => "policy_digest",
        acl: String => "acl",
        expires_at: DateTime<Utc> => "expires_at",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct McpCredentials => "mcp_credentials" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        prefix: String => "prefix",
        verifier_hash: String => "verifier_hash",
        generation: u64 => "generation",
        aggregate_version: u64 => "aggregate_version",
        expires_at: DateTime<Utc> => "expires_at",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        revoked_at: Option<DateTime<Utc>> => "revoked_at",
    }
}

orm_table! {
    pub(super) struct McpGatewaySnapshotPublicationScopes => "mcp_gateway_snapshot_publication_scopes" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        gateway_scope_id: Uuid => "gateway_scope_id",
        node_id: Uuid => "node_id",
        gateway_revision: u64 => "gateway_revision",
        scope_aggregate_version: u64 => "scope_aggregate_version",
        membership_generation: u64 => "membership_generation",
        receiving_member: bool => "receiving_member",
        mcp_route_count: u32 => "mcp_route_count",
    }
}

orm_table! {
    pub(super) struct McpCredentialDeliveryReceipts => "mcp_credential_delivery_receipts" {
        credential_id: Uuid => "credential_id",
        organization_id: Uuid => "organization_id",
        generation: u64 => "generation",
        key_id: String => "key_id",
        ciphertext: String => "ciphertext",
        expires_at: DateTime<Utc> => "expires_at",
        created_at: DateTime<Utc> => "created_at",
    }
}

orm_table! {
    pub(super) struct Workloads => "workloads" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        desired_state: String => "desired_state",
        active_revision_id: Option<Uuid> => "active_revision_id",
        aggregate_version: u64 => "aggregate_version",
    }
}
