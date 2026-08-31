use a3s_acl::builder::{boolean, integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document, Value};
use a3s_cloud_contracts::{
    McpGatewayProjection, McpGrantProjection, McpRoutePolicyProjection,
    McpServiceProfileProjection, McpTargetProjection,
};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledMcpGatewayProjection {
    /// Deterministic top-level `mcp` ACL block.
    pub acl: String,
    /// Canonical semantic digest produced by `a3s-acl`.
    pub canonical_digest: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct McpGatewayProjectionCompiler;

impl McpGatewayProjectionCompiler {
    pub fn compile(
        &self,
        projection: &McpGatewayProjection,
    ) -> Result<CompiledMcpGatewayProjection, String> {
        self.compile_at(projection, Utc::now())
    }

    pub fn compile_at(
        &self,
        projection: &McpGatewayProjection,
        now: DateTime<Utc>,
    ) -> Result<CompiledMcpGatewayProjection, String> {
        projection.validate(now)?;
        let document = Document {
            blocks: vec![mcp_block(projection)?],
        };
        let acl = generate_acl(&document);
        let reparsed = parse_acl(&acl)
            .map_err(|error| format!("generated MCP Gateway ACL is invalid: {error}"))?;
        let canonical_digest = canonical_digest(&reparsed).map_err(|error| {
            format!("generated MCP Gateway ACL is not canonicalizable: {error}")
        })?;
        Ok(CompiledMcpGatewayProjection {
            acl,
            canonical_digest,
        })
    }
}

fn mcp_block(projection: &McpGatewayProjection) -> Result<a3s_acl::Block, String> {
    let mut block =
        BlockBuilder::new("mcp").attr("expires_at", string(&projection.expires_at.to_rfc3339()));

    let mut profiles = projection.profiles.iter().collect::<Vec<_>>();
    profiles.sort_by(|left, right| left.profile_digest.cmp(&right.profile_digest));
    for profile in profiles {
        block = block.nested_block(profile_block(profile)?);
    }

    let mut credentials = projection.credentials.iter().collect::<Vec<_>>();
    credentials.sort_by_key(|credential| credential.credential_id);
    for credential in credentials {
        block = block.nested_block(
            BlockBuilder::new("credentials")
                .label(&credential.credential_id.to_string())
                .attr(
                    "environment_id",
                    string(&credential.environment_id.to_string()),
                )
                .attr("audience", string(&credential.audience))
                .attr("prefix", string(&credential.prefix))
                .attr("verifier_hash", string(credential.verifier_hash()))
                .attr("generation", acl_u64(credential.generation)?)
                .attr("expires_at", string(&credential.expires_at.to_rfc3339()))
                .attr("revoked", boolean(credential.revoked))
                .build(),
        );
    }

    let mut routes = projection.routes.iter().collect::<Vec<_>>();
    routes.sort_by_key(|route| route.route_id);
    for route in routes {
        block = block.nested_block(route_block(route)?);
    }
    Ok(block.build())
}

fn profile_block(profile: &McpServiceProfileProjection) -> Result<a3s_acl::Block, String> {
    Ok(BlockBuilder::new("profiles")
        .label(&profile.profile_digest)
        .attr(
            "protocol_versions",
            sorted_strings(&profile.protocol_versions),
        )
        .attr("path", string(&profile.path))
        .attr("request_sse", boolean(profile.request_sse))
        .attr("subscriptions", boolean(profile.subscriptions))
        .attr("max_request_bytes", acl_u64(profile.max_request_bytes)?)
        .attr("max_response_bytes", acl_u64(profile.max_response_bytes)?)
        .build())
}

fn route_block(route: &McpRoutePolicyProjection) -> Result<a3s_acl::Block, String> {
    let mut block = BlockBuilder::new("routes")
        .label(&route.route_id.to_string())
        .attr("router", string(&route.router))
        .attr("environment_id", string(&route.environment_id.to_string()))
        .attr("policy_revision", acl_u64(route.policy_revision)?)
        .attr("profile_digest", string(&route.profile_digest))
        .attr("allowed_origins", sorted_strings(&route.allowed_origins))
        .attr("max_header_bytes", acl_u64(route.max_header_bytes)?)
        .attr("max_request_bytes", acl_u64(route.max_request_bytes)?)
        .attr("max_response_bytes", acl_u64(route.max_response_bytes)?)
        .attr(
            "first_response_timeout",
            string(&route.first_response_timeout),
        )
        .attr("stream_idle_timeout", string(&route.stream_idle_timeout))
        .attr("stream_total_timeout", string(&route.stream_total_timeout))
        .attr("drain_timeout", string(&route.drain_timeout))
        .attr("telemetry_names", sorted_strings(&route.telemetry_names));

    let mut targets = route.targets.iter().collect::<Vec<_>>();
    targets.sort_by_key(|target| (target.priority, target.target_id));
    for target in targets {
        block = block.nested_block(target_block(target)?);
    }

    let mut grants = route.grants.iter().collect::<Vec<_>>();
    grants.sort_by_key(|grant| grant.credential_id);
    for grant in grants {
        block = block.nested_block(grant_block(grant)?);
    }
    Ok(block.build())
}

fn target_block(target: &McpTargetProjection) -> Result<a3s_acl::Block, String> {
    Ok(BlockBuilder::new("targets")
        .label(&target.target_id.to_string())
        .attr("node_id", string(&target.node_id.to_string()))
        .attr(
            "asset_release_id",
            string(&target.asset_release_id.to_string()),
        )
        .attr("unit_id", string(&target.unit_id))
        .attr("generation", acl_u64(target.generation)?)
        .attr("service", string(&target.service))
        .attr("endpoint", string(&target.endpoint))
        .attr("profile_digest", string(&target.profile_digest))
        .attr("priority", integer(i64::from(target.priority)))
        .attr("weight", integer(i64::from(target.weight)))
        .build())
}

fn grant_block(grant: &McpGrantProjection) -> Result<a3s_acl::Block, String> {
    let limits = BlockBuilder::new("limits")
        .attr(
            "max_concurrent_requests",
            acl_u64(grant.limits.max_concurrent_requests)?,
        )
        .attr(
            "requests_per_minute",
            acl_u64(grant.limits.requests_per_minute)?,
        )
        .attr("request_burst", acl_u64(grant.limits.request_burst)?)
        .build();
    Ok(BlockBuilder::new("grants")
        .label(&grant.credential_id.to_string())
        .attr(
            "credential_generation",
            acl_u64(grant.credential_generation)?,
        )
        .attr("methods", sorted_strings(&grant.methods))
        .attr("names", sorted_strings(&grant.names))
        .nested_block(limits)
        .build())
}

fn sorted_strings(values: &[String]) -> Value {
    let mut values = values.iter().map(String::as_str).collect::<Vec<_>>();
    values.sort_unstable();
    list(values.into_iter().map(string).collect())
}

fn acl_u64(value: u64) -> Result<Value, String> {
    let value = i64::try_from(value)
        .map_err(|_| "MCP projection integer is not representable by ACL".to_string())?;
    Ok(integer(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        McpCredentialProjection, McpLimitsProjection, MCP_CREDENTIAL_AUDIENCE,
        MCP_GATEWAY_PROJECTION_SCHEMA, MCP_PROTOCOL_VERSION,
    };
    use chrono::{Duration, TimeZone};
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    const PROFILE_DIGEST: &str =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn fixture_projection() -> McpGatewayProjection {
        let expires_at = Utc
            .with_ymd_and_hms(2099, 1, 1, 0, 0, 0)
            .single()
            .expect("valid fixture expiry");
        let credential_expiry = Utc
            .with_ymd_and_hms(2098, 12, 31, 23, 0, 0)
            .single()
            .expect("valid credential expiry");
        let environment_id =
            Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("environment UUID");
        let credential_id =
            Uuid::parse_str("33333333-3333-4333-8333-333333333333").expect("credential UUID");
        McpGatewayProjection {
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
                "a3s_mcp_abc12345",
                "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                7,
                credential_expiry,
                false,
            )],
            routes: vec![McpRoutePolicyProjection {
                route_id: Uuid::parse_str("44444444-4444-4444-8444-444444444444")
                    .expect("route UUID"),
                router: "mcp".into(),
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
                    target_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555")
                        .expect("target UUID"),
                    node_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666")
                        .expect("node UUID"),
                    asset_release_id: Uuid::parse_str(
                        "77777777-7777-4777-8777-777777777777",
                    )
                    .expect("release UUID"),
                    unit_id: "workload:mcp-weather:replica:1".into(),
                    generation: 3,
                    service: "mcp-target-1".into(),
                    endpoint: "http://127.0.0.1:8000/".into(),
                    profile_digest: PROFILE_DIGEST.into(),
                    priority: 0,
                    weight: 100,
                }],
                grants: vec![McpGrantProjection {
                    credential_id,
                    credential_generation: 7,
                    methods: vec![
                        "tools/call".into(),
                        "server/discover".into(),
                        "tools/list".into(),
                    ],
                    names: vec!["weather".into()],
                    limits: McpLimitsProjection {
                        max_concurrent_requests: 8,
                        requests_per_minute: 120,
                        request_burst: 16,
                    },
                }],
            }],
        }
    }

    #[test]
    fn generates_the_frozen_mcp01_acl_fixture_with_a_canonical_digest() {
        let now = Utc
            .with_ymd_and_hms(2098, 12, 31, 0, 0, 0)
            .single()
            .expect("valid fixture now");
        let compiled = McpGatewayProjectionCompiler
            .compile_at(&fixture_projection(), now)
            .expect("compiled projection");
        assert_eq!(
            compiled.acl,
            include_str!("../../../../../../contracts/mcp0.1/mcp-policy.acl")
                .trim_end_matches(['\r', '\n'])
        );
        assert!(compiled.canonical_digest.starts_with("sha256:"));
    }

    #[test]
    fn canonical_output_is_independent_of_input_set_order() {
        let now = Utc::now() - Duration::seconds(1);
        let mut first = fixture_projection();
        first.expires_at = Utc::now() + Duration::hours(1);
        let mut second = first.clone();
        second.routes[0].grants[0].methods.reverse();
        second.routes[0].allowed_origins.reverse();
        assert_eq!(
            McpGatewayProjectionCompiler
                .compile_at(&first, now)
                .expect("first projection"),
            McpGatewayProjectionCompiler
                .compile_at(&second, now)
                .expect("second projection")
        );
    }

    #[test]
    fn frozen_cross_repository_fixtures_share_one_profile_binding() {
        const POLICY: &[u8] = include_bytes!("../../../../../../contracts/mcp0.1/mcp-policy.acl");
        const RUNTIME: &[u8] =
            include_bytes!("../../../../../../contracts/mcp0.1/runtime-unit-spec.json");
        const GATEWAY: &[u8] =
            include_bytes!("../../../../../../contracts/mcp0.1/gateway-snapshot.acl");
        assert_eq!(
            format!("{:x}", Sha256::digest(POLICY)),
            "5f30512ff696a7bbc25417819c2432027de20123f229d8ddbd29298d0da821e0"
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(RUNTIME)),
            "f7e7b84867c5c646ad5a02d9e7be65a23ce7f6eb813ae1e1bf4786d4db602f4a"
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(GATEWAY)),
            "a3c12ad36e8c2c06787ec1b42899fa5cea5a10f00ce2ab42c1abaddec50036a5"
        );

        let runtime: a3s_runtime::contract::RuntimeUnitSpec =
            serde_json::from_slice(RUNTIME).expect("Runtime fixture");
        runtime.validate().expect("valid Runtime fixture");
        assert_eq!(
            runtime.semantics_profile_digest.as_deref(),
            Some(PROFILE_DIGEST)
        );
        assert!(runtime.identity_attachment_digest.is_none());

        let gateway = parse_acl(std::str::from_utf8(GATEWAY).expect("Gateway fixture UTF-8"))
            .expect("Gateway fixture ACL");
        let mcp = gateway
            .blocks
            .iter()
            .find(|block| block.name == "mcp")
            .expect("MCP block");
        let profile = mcp
            .blocks
            .iter()
            .find(|block| block.name == "profiles")
            .expect("profile block");
        assert_eq!(
            profile.labels.first().map(String::as_str),
            Some(PROFILE_DIGEST)
        );
        let route = mcp
            .blocks
            .iter()
            .find(|block| block.name == "routes")
            .expect("route block");
        assert_eq!(
            route
                .attributes
                .get("profile_digest")
                .and_then(Value::as_str),
            Some(PROFILE_DIGEST)
        );
        assert_eq!(
            route
                .blocks
                .iter()
                .find(|block| block.name == "targets")
                .and_then(|target| target.attributes.get("profile_digest"))
                .and_then(Value::as_str),
            Some(PROFILE_DIGEST)
        );
    }
}
