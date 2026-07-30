use crate::modules::edge::infrastructure::{
    McpCredentialProjectionVersion, PlannedMcpGatewayProjection,
};
use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::shared_kernel::domain::McpCredentialId;
use a3s_cloud_contracts::{
    McpCredentialProjection, McpGatewayProjection, McpRoutePolicyProjection,
    McpServiceProfileProjection, MCP_GATEWAY_PROJECTION_SCHEMA,
};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Combines independently planned routes into one complete snapshot for one
/// physical Gateway.
#[derive(Debug, Clone, Copy, Default)]
pub struct McpGatewayProjectionAssembler;

impl McpGatewayProjectionAssembler {
    pub fn assemble(
        &self,
        fragments: Vec<PlannedMcpGatewayProjection>,
        observed_at: DateTime<Utc>,
    ) -> Result<PlannedMcpGatewayProjection, String> {
        self.assemble_with_mode(fragments, observed_at, true)
    }

    /// Merges complete per-scope projections into one complete physical-node
    /// projection. Unlike [`Self::assemble`], each input may contain multiple
    /// routes, but every identity and authority definition must still agree.
    pub fn assemble_complete(
        &self,
        fragments: Vec<PlannedMcpGatewayProjection>,
        observed_at: DateTime<Utc>,
    ) -> Result<PlannedMcpGatewayProjection, String> {
        self.assemble_with_mode(fragments, observed_at, false)
    }

    fn assemble_with_mode(
        &self,
        fragments: Vec<PlannedMcpGatewayProjection>,
        observed_at: DateTime<Utc>,
        require_one_route_fragment: bool,
    ) -> Result<PlannedMcpGatewayProjection, String> {
        if fragments.is_empty() || fragments.len() > 1_000 {
            return Err(
                "MCP Gateway assembly requires between one and 1000 bounded fragments".into(),
            );
        }
        let observed_at = canonical_timestamp(observed_at);
        let gateway_node_id = fragments[0].gateway_node_id();
        let mut expires_at: Option<DateTime<Utc>> = None;
        let mut profiles = BTreeMap::<String, McpServiceProfileProjection>::new();
        let mut credentials = BTreeMap::<Uuid, McpCredentialProjection>::new();
        let mut credential_versions =
            BTreeMap::<McpCredentialId, McpCredentialProjectionVersion>::new();
        let mut routes = BTreeMap::<Uuid, McpRoutePolicyProjection>::new();
        let mut routers = BTreeSet::new();

        for fragment in fragments {
            if fragment.gateway_node_id() != gateway_node_id {
                return Err(
                    "MCP Gateway assembly cannot mix different physical Gateway nodes".into(),
                );
            }
            let (_, projection, fragment_credential_versions) = fragment.into_parts();
            projection.validate(observed_at)?;
            if require_one_route_fragment {
                validate_one_route_fragment(&projection)?;
            }
            expires_at = Some(
                expires_at
                    .map(|current| current.min(projection.expires_at))
                    .unwrap_or(projection.expires_at),
            );

            for profile in projection.profiles {
                match profiles.get(&profile.profile_digest) {
                    Some(existing) if existing != &profile => {
                        return Err(
                            "MCP Gateway assembly found conflicting definitions for one profile"
                                .into(),
                        );
                    }
                    Some(_) => {}
                    None => {
                        profiles.insert(profile.profile_digest.clone(), profile);
                    }
                }
            }
            for credential in projection.credentials {
                match credentials.get(&credential.credential_id) {
                    Some(existing) if existing != &credential => {
                        return Err(
                            "MCP Gateway assembly found conflicting definitions for one credential"
                                .into(),
                        );
                    }
                    Some(_) => {}
                    None => {
                        credentials.insert(credential.credential_id, credential);
                    }
                }
            }
            for version in fragment_credential_versions {
                match credential_versions.get(&version.credential_id()) {
                    Some(existing) if existing != &version => {
                        return Err(
                            "MCP Gateway assembly found conflicting versions for one credential"
                                .into(),
                        );
                    }
                    Some(_) => {}
                    None => {
                        credential_versions.insert(version.credential_id(), version);
                    }
                }
            }
            for route in projection.routes {
                if !routers.insert(route.router.clone()) {
                    return Err("MCP Gateway assembly contains a duplicate router".into());
                }
                if routes.insert(route.route_id, route).is_some() {
                    return Err("MCP Gateway assembly contains a duplicate route".into());
                }
            }
        }

        PlannedMcpGatewayProjection::new(
            gateway_node_id,
            McpGatewayProjection {
                schema: MCP_GATEWAY_PROJECTION_SCHEMA.into(),
                expires_at: expires_at
                    .ok_or_else(|| "MCP Gateway assembly has no expiry".to_string())?,
                profiles: profiles.into_values().collect(),
                credentials: credentials.into_values().collect(),
                routes: routes.into_values().collect(),
            },
            credential_versions.into_values().collect(),
            observed_at,
        )
    }
}

fn validate_one_route_fragment(projection: &McpGatewayProjection) -> Result<(), String> {
    if projection.routes.len() != 1 || projection.profiles.len() != 1 {
        return Err(
            "MCP Gateway assembly inputs must each contain exactly one route and one profile"
                .into(),
        );
    }
    let route = &projection.routes[0];
    if projection.profiles[0].profile_digest != route.profile_digest {
        return Err("MCP Gateway assembly fragment profile does not belong to its route".into());
    }
    let grant_ids = route
        .grants
        .iter()
        .map(|grant| grant.credential_id)
        .collect::<BTreeSet<_>>();
    let credential_ids = projection
        .credentials
        .iter()
        .map(|credential| credential.credential_id)
        .collect::<BTreeSet<_>>();
    if grant_ids != credential_ids {
        return Err(
            "MCP Gateway assembly fragment must contain exactly its route credentials".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::NodeId;
    use a3s_cloud_contracts::{
        McpGrantProjection, McpLimitsProjection, McpTargetProjection, MCP_CREDENTIAL_AUDIENCE,
        MCP_PROTOCOL_VERSION,
    };
    use chrono::{Duration, TimeZone};

    const PROFILE_DIGEST: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2098, 12, 31, 0, 0, 0)
            .single()
            .expect("fixture time")
    }

    fn uuid(value: &str) -> Uuid {
        Uuid::parse_str(value).expect("fixture UUID")
    }

    fn fragment(
        gateway_node_id: NodeId,
        route_id: Uuid,
        router: &str,
        target_id: Uuid,
        service: &str,
        endpoint: &str,
        expires_at: DateTime<Utc>,
    ) -> PlannedMcpGatewayProjection {
        let environment_id = uuid("22222222-2222-4222-8222-222222222222");
        let credential_id = uuid("33333333-3333-4333-8333-333333333333");
        let projection = McpGatewayProjection {
            schema: MCP_GATEWAY_PROJECTION_SCHEMA.into(),
            expires_at,
            profiles: vec![McpServiceProfileProjection {
                profile_digest: PROFILE_DIGEST.into(),
                protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
                path: "/mcp".into(),
                request_sse: true,
                subscriptions: true,
                max_request_bytes: 1_048_576,
                max_response_bytes: 8_388_608,
            }],
            credentials: vec![McpCredentialProjection::new(
                credential_id,
                environment_id,
                MCP_CREDENTIAL_AUDIENCE,
                "a3s_mcp_abc12345def67890",
                VERIFIER,
                7,
                now() + Duration::hours(2),
                false,
            )],
            routes: vec![McpRoutePolicyProjection {
                route_id,
                router: router.into(),
                environment_id,
                policy_revision: 11,
                profile_digest: PROFILE_DIGEST.into(),
                allowed_origins: vec!["https://console.example.com".into()],
                max_header_bytes: 32_768,
                max_request_bytes: 524_288,
                max_response_bytes: 4_194_304,
                first_response_timeout: "30s".into(),
                stream_idle_timeout: "2m".into(),
                stream_total_timeout: "30m".into(),
                drain_timeout: "30s".into(),
                telemetry_names: vec!["weather".into()],
                targets: vec![McpTargetProjection {
                    target_id,
                    node_id: gateway_node_id.as_uuid(),
                    asset_release_id: uuid("77777777-7777-4777-8777-777777777777"),
                    unit_id: format!("mcp-unit-{target_id}"),
                    generation: 3,
                    service: service.into(),
                    endpoint: endpoint.into(),
                    profile_digest: PROFILE_DIGEST.into(),
                    priority: 0,
                    weight: 1,
                }],
                grants: vec![McpGrantProjection {
                    credential_id,
                    credential_generation: 7,
                    methods: vec!["server/discover".into(), "tools/call".into()],
                    names: vec!["weather".into()],
                    limits: McpLimitsProjection {
                        max_concurrent_requests: 8,
                        requests_per_minute: 120,
                        request_burst: 16,
                    },
                }],
            }],
        };
        PlannedMcpGatewayProjection::new(
            gateway_node_id,
            projection,
            vec![McpCredentialProjectionVersion::new(
                McpCredentialId::from_uuid(credential_id),
                7,
                7,
            )
            .expect("credential version")],
            now(),
        )
        .expect("one-route fragment")
    }

    fn fragments(
        gateway_node_id: NodeId,
    ) -> (PlannedMcpGatewayProjection, PlannedMcpGatewayProjection) {
        (
            fragment(
                gateway_node_id,
                uuid("44444444-4444-4444-8444-444444444444"),
                "mcp-route-b",
                uuid("55555555-5555-4555-8555-555555555555"),
                "mcp-target-b",
                "http://127.0.0.1:49152/",
                now() + Duration::hours(1),
            ),
            fragment(
                gateway_node_id,
                uuid("11111111-1111-4111-8111-111111111111"),
                "mcp-route-a",
                uuid("66666666-6666-4666-8666-666666666666"),
                "mcp-target-a",
                "http://127.0.0.1:49153/",
                now() + Duration::minutes(30),
            ),
        )
    }

    #[test]
    fn assembles_one_canonical_snapshot_and_deduplicates_shared_authority() {
        let gateway_node_id = NodeId::new();
        let (later_route, earlier_route) = fragments(gateway_node_id);

        let assembled = McpGatewayProjectionAssembler
            .assemble(vec![later_route.clone(), earlier_route.clone()], now())
            .expect("assembled projection");
        let reversed = McpGatewayProjectionAssembler
            .assemble(vec![earlier_route, later_route], now())
            .expect("reversed projection");

        assert_eq!(assembled, reversed);
        assert_eq!(assembled.gateway_node_id(), gateway_node_id);
        let projection = assembled.projection();
        assert_eq!(projection.expires_at, now() + Duration::minutes(30));
        assert_eq!(projection.profiles.len(), 1);
        assert_eq!(projection.credentials.len(), 1);
        assert_eq!(projection.routes.len(), 2);
        assert!(projection.routes[0].route_id < projection.routes[1].route_id);
        assert!(projection
            .routes
            .iter()
            .flat_map(|route| &route.targets)
            .all(|target| target.node_id == gateway_node_id.as_uuid()));
        projection.validate(now()).expect("complete projection");
    }

    #[test]
    fn rejects_fragments_for_different_physical_gateways() {
        let first_node = NodeId::new();
        let second_node = NodeId::new();
        let (first, _) = fragments(first_node);
        let (_, second) = fragments(second_node);

        assert!(McpGatewayProjectionAssembler
            .assemble(vec![first, second], now())
            .expect_err("mixed nodes")
            .contains("different physical Gateway"));
    }

    #[test]
    fn rejects_conflicting_shared_profile_or_credential_authority() {
        let gateway_node_id = NodeId::new();
        let (first, second) = fragments(gateway_node_id);
        let mut conflicting_profile = second.clone().into_projection();
        conflicting_profile.profiles[0].max_response_bytes -= 1;
        let conflicting_profile = PlannedMcpGatewayProjection::new(
            gateway_node_id,
            conflicting_profile,
            second.credential_versions().to_vec(),
            now(),
        )
        .expect("valid conflicting profile fragment");
        assert!(McpGatewayProjectionAssembler
            .assemble(vec![first.clone(), conflicting_profile], now())
            .expect_err("conflicting profile")
            .contains("conflicting definitions"));

        let credential_versions = second.credential_versions().to_vec();
        let mut conflicting_credential = second.clone().into_projection();
        conflicting_credential.credentials[0].prefix = "a3s_mcp_def67890abc12345".into();
        let conflicting_credential = PlannedMcpGatewayProjection::new(
            gateway_node_id,
            conflicting_credential,
            credential_versions,
            now(),
        )
        .expect("valid conflicting credential fragment");
        assert!(McpGatewayProjectionAssembler
            .assemble(vec![first.clone(), conflicting_credential], now())
            .expect_err("conflicting credential")
            .contains("conflicting definitions"));

        let credential_id =
            McpCredentialId::from_uuid(second.projection().credentials[0].credential_id);
        let conflicting_version = PlannedMcpGatewayProjection::new(
            gateway_node_id,
            second.into_projection(),
            vec![McpCredentialProjectionVersion::new(credential_id, 7, 8)
                .expect("conflicting version")],
            now(),
        )
        .expect("valid newer authority fragment");
        assert!(McpGatewayProjectionAssembler
            .assemble(vec![first, conflicting_version], now())
            .expect_err("conflicting credential version")
            .contains("conflicting versions"));
    }

    #[test]
    fn rejects_duplicate_route_or_router_ownership() {
        let gateway_node_id = NodeId::new();
        let (first, second) = fragments(gateway_node_id);
        let mut duplicate_route = second.clone().into_projection();
        duplicate_route.routes[0].route_id = first.projection().routes[0].route_id;
        let duplicate_route = PlannedMcpGatewayProjection::new(
            gateway_node_id,
            duplicate_route,
            second.credential_versions().to_vec(),
            now(),
        )
        .expect("valid duplicate route fragment");
        assert!(McpGatewayProjectionAssembler
            .assemble(vec![first.clone(), duplicate_route], now())
            .expect_err("duplicate route")
            .contains("duplicate route"));

        let credential_versions = second.credential_versions().to_vec();
        let mut duplicate_router = second.into_projection();
        duplicate_router.routes[0].router = first.projection().routes[0].router.clone();
        let duplicate_router = PlannedMcpGatewayProjection::new(
            gateway_node_id,
            duplicate_router,
            credential_versions,
            now(),
        )
        .expect("valid duplicate router fragment");
        assert!(McpGatewayProjectionAssembler
            .assemble(vec![first, duplicate_router], now())
            .expect_err("duplicate router")
            .contains("duplicate router"));
    }

    #[test]
    fn rejects_a_preassembled_snapshot_as_a_route_fragment() {
        let gateway_node_id = NodeId::new();
        let (first, second) = fragments(gateway_node_id);
        let assembled = McpGatewayProjectionAssembler
            .assemble(vec![first, second], now())
            .expect("assembled projection");

        assert!(McpGatewayProjectionAssembler
            .assemble(vec![assembled], now())
            .expect_err("nested complete snapshot")
            .contains("exactly one route and one profile"));
    }
}
