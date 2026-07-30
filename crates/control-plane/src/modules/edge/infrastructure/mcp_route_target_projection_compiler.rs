use crate::modules::assets::domain::McpServiceProfile;
use crate::modules::edge::domain::services::ResolvedRouteTarget;
use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::workloads::domain::entities::WorkloadRevision;
use a3s_cloud_contracts::{McpRoutePolicyProjection, McpTargetProjection};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use uuid::Uuid;

const MAX_MCP_ROUTE_TARGETS: usize = 64;
const MCP_ROUTE_TARGET_NAMESPACE: Uuid = Uuid::from_u128(0xb434_8763_d426_5a01_a661_d65d_5a18_8d82);

/// One Runtime-observed target plus the mutable traffic controls Cloud owns.
///
/// Endpoint, node, revision, Unit, and generation are deliberately retained
/// inside `ResolvedRouteTarget`; callers may only supply priority and weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRouteTargetCandidate {
    resolved: ResolvedRouteTarget,
    priority: u32,
    weight: u32,
}

impl McpRouteTargetCandidate {
    pub fn new(resolved: ResolvedRouteTarget, priority: u32, weight: u32) -> Result<Self, String> {
        if weight == 0 {
            return Err("MCP route target weight must be positive".into());
        }
        Ok(Self {
            resolved,
            priority,
            weight,
        })
    }

    pub const fn resolved(&self) -> &ResolvedRouteTarget {
        &self.resolved
    }

    pub const fn priority(&self) -> u32 {
        self.priority
    }

    pub const fn weight(&self) -> u32 {
        self.weight
    }
}

/// Compiles exact Runtime evidence into the target portion of a Gateway route.
///
/// Product identity comes only from the immutable Workload revision binding,
/// while endpoint identity comes only from a validated Runtime observation.
/// The compiler never accepts either field as a free-form projection input.
#[derive(Debug, Clone, Copy, Default)]
pub struct McpRouteTargetProjectionCompiler;

impl McpRouteTargetProjectionCompiler {
    pub fn compile(
        &self,
        policy: &McpRoutePolicy,
        profile: &McpServiceProfile,
        revision: &WorkloadRevision,
        router: impl Into<String>,
        mut candidates: Vec<McpRouteTargetCandidate>,
    ) -> Result<McpRoutePolicyProjection, String> {
        let router = router.into();
        validate_router(&router)?;
        let binding = revision
            .mcp_binding()
            .ok_or_else(|| "MCP route requires a release-bound Workload revision".to_owned())?;
        let policy_spec = policy.spec();

        if revision.id.as_uuid().is_nil()
            || revision.workload_id.as_uuid().is_nil()
            || revision.generation == 0
        {
            return Err("MCP route Workload revision identity is invalid".into());
        }
        if binding.organization_id() != policy_spec.organization_id
            || revision.workload_id != policy_spec.workload_id
            || binding.asset_id() != policy_spec.asset_id
            || binding.asset_release_id() != policy_spec.asset_release_id
            || binding.profile_digest() != &policy_spec.profile_digest
            || binding.profile_digest() != profile.digest()
        {
            return Err(
                "MCP route policy, Workload revision, release, and Service profile differ".into(),
            );
        }

        validate_bound_template(revision, profile)?;
        if candidates.is_empty() || candidates.len() > MAX_MCP_ROUTE_TARGETS {
            return Err("MCP route requires between one and 64 healthy Runtime targets".into());
        }

        candidates.sort_by_key(|candidate| {
            (
                candidate.priority,
                candidate.resolved.node_id,
                candidate.resolved.target.runtime_generation,
            )
        });
        validate_candidates(revision, profile, &candidates)?;

        let targets = candidates
            .into_iter()
            .map(|candidate| {
                let target_id = deterministic_target_id(
                    policy_spec.route_id.as_uuid(),
                    candidate.resolved.node_id.as_uuid(),
                    &candidate.resolved.target.runtime_unit_id,
                    candidate.resolved.target.runtime_generation,
                );
                McpTargetProjection {
                    target_id,
                    node_id: candidate.resolved.node_id.as_uuid(),
                    asset_release_id: binding.asset_release_id().as_uuid(),
                    unit_id: candidate.resolved.target.runtime_unit_id,
                    generation: candidate.resolved.target.runtime_generation,
                    service: format!("mcp-target-{}", target_id.simple()),
                    endpoint: candidate.resolved.target.upstream.as_str().to_owned(),
                    profile_digest: binding.profile_digest().to_string(),
                    priority: candidate.priority,
                    weight: candidate.weight,
                }
            })
            .collect();

        Ok(policy.gateway_projection(router, targets))
    }
}

fn validate_router(router: &str) -> Result<(), String> {
    if router.is_empty()
        || router.len() > 255
        || router.trim() != router
        || router.chars().any(char::is_control)
    {
        return Err("MCP router is invalid".into());
    }
    Ok(())
}

fn validate_bound_template(
    revision: &WorkloadRevision,
    profile: &McpServiceProfile,
) -> Result<(), String> {
    McpServiceProfile::restore(profile.canonical_acl(), profile.digest().as_str())?;
    let template = revision.resolved_template()?;
    template.validate()?;
    if !template
        .ports
        .iter()
        .any(|port| port.name == profile.spec().runtime_port)
    {
        return Err("MCP Workload does not declare the bound profile Runtime port".into());
    }
    let health = template
        .health
        .as_ref()
        .ok_or_else(|| "MCP Workload requires the bound profile HTTP health check".to_owned())?;
    if health.port_name != profile.spec().runtime_port || health.path != profile.spec().health_path
    {
        return Err("MCP Workload health check differs from the bound Service profile".into());
    }
    Ok(())
}

fn validate_candidates(
    revision: &WorkloadRevision,
    profile: &McpServiceProfile,
    candidates: &[McpRouteTargetCandidate],
) -> Result<(), String> {
    let expected_unit_id = revision.runtime_unit_id();
    let mut nodes = HashSet::new();
    let mut priorities = BTreeSet::new();
    let mut weights = BTreeMap::<u32, u64>::new();

    for candidate in candidates {
        let resolved = &candidate.resolved;
        let target = &resolved.target;
        target.validate_for(resolved.workload_id)?;
        if resolved.node_id.as_uuid().is_nil()
            || !nodes.insert(resolved.node_id)
            || resolved.workload_id != revision.workload_id
            || target.workload_revision_id != revision.id
            || target.runtime_unit_id != expected_unit_id
            || target.runtime_generation != revision.generation
            || target.port_name.as_str() != profile.spec().runtime_port
        {
            return Err(
                "MCP route target differs from its exact Workload revision or Runtime evidence"
                    .into(),
            );
        }
        if candidate.weight == 0 {
            return Err("MCP route target weight must be positive".into());
        }
        priorities.insert(candidate.priority);
        let weight = weights.entry(candidate.priority).or_default();
        *weight = weight
            .checked_add(u64::from(candidate.weight))
            .ok_or_else(|| "MCP route target weight total overflowed".to_owned())?;
        if *weight > u64::from(u32::MAX) {
            return Err("MCP route target weight total exceeds u32".into());
        }
    }

    if priorities
        .iter()
        .copied()
        .enumerate()
        .any(|(expected, actual)| usize::try_from(actual) != Ok(expected))
    {
        return Err("MCP route target priorities must be contiguous from zero".into());
    }
    Ok(())
}

fn deterministic_target_id(route_id: Uuid, node_id: Uuid, unit_id: &str, generation: u64) -> Uuid {
    let identity =
        format!("route={route_id};node={node_id};unit={unit_id};generation={generation}");
    Uuid::new_v5(&MCP_ROUTE_TARGET_NAMESPACE, identity.as_bytes())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::modules::assets::domain::{McpServiceProfile, McpServiceProfileSpec};
    use crate::modules::edge::domain::services::ResolvedRouteTarget;
    use crate::modules::edge::domain::{
        McpRoutePolicySpec, RouteHostname, RoutePortName, RouteTarget, UpstreamEndpoint,
    };
    use crate::modules::shared_kernel::domain::{
        AssetId, AssetReleaseId, DomainClaimId, EnvironmentId, GatewayScopeId, NodeId,
        OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use crate::modules::workloads::domain::entities::{
        HttpHealthCheck, McpWorkloadRevisionBinding, OciArtifact, ServicePort, ServiceProcess,
        ServiceResources, ServiceTemplate, WorkloadRevision,
    };
    use a3s_cloud_contracts::{McpGrantProjection, McpLimitsProjection, MCP_PROTOCOL_VERSION};
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use std::collections::BTreeMap;

    pub(in crate::modules::edge::infrastructure) struct Fixture {
        pub(in crate::modules::edge::infrastructure) profile: McpServiceProfile,
        pub(in crate::modules::edge::infrastructure) policy: McpRoutePolicy,
        pub(in crate::modules::edge::infrastructure) revision: WorkloadRevision,
    }

    fn uuid(value: &str) -> Uuid {
        Uuid::parse_str(value).expect("UUID")
    }

    pub(in crate::modules::edge::infrastructure) fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("time")
    }

    pub(in crate::modules::edge::infrastructure) fn fixture() -> Fixture {
        let profile = McpServiceProfile::from_spec(McpServiceProfileSpec {
            protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
            endpoint_path: "/mcp".into(),
            runtime_port: "mcp".into(),
            health_path: "/health".into(),
            request_sse: true,
            subscriptions: true,
            server_discover: true,
            expected_capabilities: vec!["subscriptions".into(), "tools".into()],
            max_request_bytes: 1_048_576,
            max_response_bytes: 8_388_608,
            max_stream_seconds: 3_600,
        })
        .expect("profile");
        let organization_id =
            OrganizationId::from_uuid(uuid("11111111-1111-4111-8111-111111111111"));
        let workload_id = WorkloadId::from_uuid(uuid("22222222-2222-4222-8222-222222222222"));
        let asset_id = AssetId::from_uuid(uuid("33333333-3333-4333-8333-333333333333"));
        let asset_release_id =
            AssetReleaseId::from_uuid(uuid("44444444-4444-4444-8444-444444444444"));
        let revision_id =
            WorkloadRevisionId::from_uuid(uuid("55555555-5555-4555-8555-555555555555"));
        let digest = format!("sha256:{}", "a".repeat(64));
        let mut revision = WorkloadRevision::create(
            revision_id,
            workload_id,
            7,
            ServiceTemplate {
                artifact: OciArtifact {
                    uri: format!("oci://registry.example/mcp@{digest}"),
                    digest,
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: ServiceProcess {
                    command: vec!["/app/mcp".into()],
                    args: vec!["serve".into()],
                    working_directory: Some("/app".into()),
                    environment: BTreeMap::new(),
                },
                secrets: Vec::new(),
                resources: ServiceResources {
                    cpu_millis: 500,
                    memory_bytes: 256 * 1024 * 1024,
                    pids: 128,
                    ephemeral_storage_bytes: Some(1024 * 1024 * 1024),
                },
                ports: vec![ServicePort {
                    name: "mcp".into(),
                    container_port: 8080,
                }],
                health: Some(HttpHealthCheck {
                    port_name: "mcp".into(),
                    path: "/health".into(),
                    interval_ms: 10_000,
                    timeout_ms: 2_000,
                    healthy_threshold: 1,
                    unhealthy_threshold: 3,
                    stabilization_window_ms: 30_000,
                }),
            },
            now(),
        )
        .expect("revision");
        revision
            .restore_mcp_binding(
                McpWorkloadRevisionBinding::restore(
                    organization_id,
                    asset_id,
                    asset_release_id,
                    profile.digest().clone(),
                )
                .expect("binding"),
                &profile,
            )
            .expect("restore binding");
        let policy = McpRoutePolicy::create(
            McpRoutePolicySpec {
                route_id: RouteId::from_uuid(uuid("66666666-6666-4666-8666-666666666666")),
                organization_id,
                project_id: ProjectId::from_uuid(uuid("77777777-7777-4777-8777-777777777777")),
                environment_id: EnvironmentId::from_uuid(uuid(
                    "88888888-8888-4888-8888-888888888888",
                )),
                gateway_scope_id: GatewayScopeId::from_uuid(uuid(
                    "99999999-9999-4999-8999-999999999999",
                )),
                domain_claim_id: DomainClaimId::from_uuid(uuid(
                    "99999999-9999-4999-8999-999999999998",
                )),
                workload_id,
                asset_id,
                asset_release_id,
                profile_digest: profile.digest().clone(),
                hostname: RouteHostname::parse("mcp.example.com").expect("hostname"),
                path: "/mcp".into(),
                tls_required: true,
                allowed_origins: vec!["https://console.example.com".into()],
                max_header_bytes: 32_768,
                max_request_bytes: 524_288,
                max_response_bytes: 4_194_304,
                first_response_timeout_seconds: 30,
                stream_idle_timeout_seconds: 120,
                stream_total_timeout_seconds: 1_800,
                drain_timeout_seconds: 30,
                telemetry_names: vec!["weather".into()],
                telemetry_events_per_minute: 10_000,
                audit_required: true,
                expires_at: now() + Duration::hours(1),
                grants: vec![McpGrantProjection {
                    credential_id: uuid("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
                    credential_generation: 1,
                    methods: vec![
                        "server/discover".into(),
                        "tools/call".into(),
                        "tools/list".into(),
                    ],
                    names: vec!["weather".into()],
                    limits: McpLimitsProjection {
                        max_concurrent_requests: 8,
                        requests_per_minute: 120,
                        request_burst: 16,
                    },
                }],
            },
            &profile,
            now(),
        )
        .expect("policy");
        Fixture {
            profile,
            policy,
            revision,
        }
    }

    pub(in crate::modules::edge::infrastructure) fn target(
        fixture: &Fixture,
        node_id: NodeId,
        port: u16,
    ) -> ResolvedRouteTarget {
        ResolvedRouteTarget {
            workload_id: fixture.revision.workload_id,
            node_id,
            target: RouteTarget::new(
                fixture.revision.workload_id,
                fixture.revision.id,
                fixture.revision.runtime_unit_id(),
                fixture.revision.generation,
                RoutePortName::parse("mcp").expect("port name"),
                UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}")).expect("endpoint"),
                now(),
            )
            .expect("route target"),
        }
    }

    #[test]
    fn compiles_deterministic_release_bound_runtime_targets() {
        let fixture = fixture();
        let first_node = NodeId::from_uuid(uuid("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"));
        let second_node = NodeId::from_uuid(uuid("cccccccc-cccc-4ccc-8ccc-cccccccccccc"));
        let first = McpRouteTargetCandidate::new(target(&fixture, first_node, 49152), 0, 70)
            .expect("first");
        let second = McpRouteTargetCandidate::new(target(&fixture, second_node, 49153), 0, 30)
            .expect("second");
        let compiler = McpRouteTargetProjectionCompiler;

        let projection = compiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                vec![second.clone(), first.clone()],
            )
            .expect("projection");
        let repeated = compiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                vec![first, second],
            )
            .expect("repeat");

        assert_eq!(projection, repeated);
        assert_eq!(projection.targets.len(), 2);
        assert_eq!(
            projection.targets[0].target_id,
            uuid("c93bb450-0696-5a02-9f34-c5ff7f467876")
        );
        assert_eq!(projection.targets[0].node_id, first_node.as_uuid());
        assert_eq!(
            projection.targets[0].asset_release_id,
            fixture
                .revision
                .mcp_binding()
                .expect("binding")
                .asset_release_id()
                .as_uuid()
        );
        assert_eq!(
            projection.targets[0].profile_digest,
            fixture.profile.digest().as_str()
        );
        assert_eq!(
            projection.targets[0].unit_id,
            fixture.revision.runtime_unit_id()
        );
        assert_eq!(
            projection.targets[0].generation,
            fixture.revision.generation
        );
        assert_eq!(projection.targets[0].endpoint, "http://127.0.0.1:49152/");
        assert!(projection.targets[0].service.starts_with("mcp-target-"));
    }

    #[test]
    fn rejects_unbound_or_mismatched_runtime_evidence() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let candidate =
            McpRouteTargetCandidate::new(target(&fixture, node_id, 49152), 0, 1).expect("target");
        let mut unbound = fixture.revision.clone();
        unbound = WorkloadRevision::create(
            unbound.id,
            unbound.workload_id,
            unbound.generation,
            unbound.resolved_template().expect("template").clone(),
            unbound.created_at,
        )
        .expect("unbound revision");
        assert!(McpRouteTargetProjectionCompiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &unbound,
                "mcp",
                vec![candidate.clone()],
            )
            .expect_err("unbound revision")
            .contains("release-bound"));

        let mut wrong_generation = candidate.clone();
        wrong_generation.resolved.target.runtime_generation += 1;
        assert!(McpRouteTargetProjectionCompiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                vec![wrong_generation],
            )
            .is_err());

        let mut wrong_revision = candidate;
        wrong_revision.resolved.target.workload_revision_id = WorkloadRevisionId::new();
        wrong_revision.resolved.target.runtime_unit_id = format!(
            "workload:{}:revision:{}",
            wrong_revision.resolved.workload_id,
            wrong_revision.resolved.target.workload_revision_id
        );
        assert!(McpRouteTargetProjectionCompiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                vec![wrong_revision],
            )
            .is_err());
    }

    #[test]
    fn rejects_invalid_target_sets_and_traffic_controls() {
        let fixture = fixture();
        let compiler = McpRouteTargetProjectionCompiler;
        assert!(compiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                Vec::new(),
            )
            .is_err());
        assert!(
            McpRouteTargetCandidate::new(target(&fixture, NodeId::new(), 49152), 0, 0).is_err()
        );

        let node_id = NodeId::new();
        let duplicate = target(&fixture, node_id, 49152);
        assert!(compiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                vec![
                    McpRouteTargetCandidate::new(duplicate.clone(), 0, 1).expect("first"),
                    McpRouteTargetCandidate::new(duplicate, 0, 1).expect("duplicate"),
                ],
            )
            .is_err());
        assert!(compiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                vec![
                    McpRouteTargetCandidate::new(target(&fixture, NodeId::new(), 49153), 1, 1,)
                        .expect("priority")
                ],
            )
            .is_err());
        assert!(compiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                vec![
                    McpRouteTargetCandidate::new(
                        target(&fixture, NodeId::new(), 49154),
                        0,
                        u32::MAX,
                    )
                    .expect("maximum"),
                    McpRouteTargetCandidate::new(target(&fixture, NodeId::new(), 49155), 0, 1,)
                        .expect("overflow"),
                ],
            )
            .is_err());
    }

    #[test]
    fn rejects_a_different_profile_or_runtime_port() {
        let fixture = fixture();
        let candidate = McpRouteTargetCandidate::new(target(&fixture, NodeId::new(), 49152), 0, 1)
            .expect("target");
        let mut different_spec = fixture.profile.spec().clone();
        different_spec
            .expected_capabilities
            .push("resources".into());
        let different = McpServiceProfile::from_spec(different_spec).expect("different profile");
        assert!(McpRouteTargetProjectionCompiler
            .compile(
                &fixture.policy,
                &different,
                &fixture.revision,
                "mcp",
                vec![candidate.clone()],
            )
            .is_err());

        let mut wrong_port = candidate;
        wrong_port.resolved.target.port_name =
            RoutePortName::parse("http").expect("wrong port name");
        assert!(McpRouteTargetProjectionCompiler
            .compile(
                &fixture.policy,
                &fixture.profile,
                &fixture.revision,
                "mcp",
                vec![wrong_port],
            )
            .is_err());
    }
}
