use super::*;

pub(in super::super) struct RouteRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    gateway_scope_id: Uuid,
    gateway_node_id: Uuid,
    hostname: String,
    path_prefix: String,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    runtime_unit_id: String,
    runtime_generation: u64,
    port_name: String,
    upstream_origin: String,
    target_observed_at: DateTime<Utc>,
    state: String,
    gateway_revision: u64,
    gateway_command_id: Uuid,
    snapshot_digest: String,
    failure: Option<String>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    activated_at: Option<DateTime<Utc>>,
    domain_claim_id: Option<Uuid>,
    domain_pattern: Option<String>,
    gateway_certificate_id: Option<Uuid>,
}

pub(in super::super) struct RouteSelection;

impl Selection for RouteSelection {
    type Output = RouteRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            Routes::id().expression(),
            Routes::organization_id().expression(),
            Routes::project_id().expression(),
            Routes::environment_id().expression(),
            Routes::gateway_scope_id().expression(),
            Routes::gateway_node_id().expression(),
            Routes::hostname().expression(),
            Routes::path_prefix().expression(),
            Routes::workload_id().expression(),
            Routes::workload_revision_id().expression(),
            Routes::runtime_unit_id().expression(),
            Routes::runtime_generation().expression(),
            Routes::port_name().expression(),
            Routes::upstream_origin().expression(),
            Routes::target_observed_at().expression(),
            Routes::state().expression(),
            Routes::gateway_revision().expression(),
            Routes::gateway_command_id().expression(),
            Routes::snapshot_digest().expression(),
            Routes::failure().expression(),
            Routes::aggregate_version().expression(),
            Routes::created_at().expression(),
            Routes::updated_at().expression(),
            Routes::activated_at().expression(),
            Routes::domain_claim_id().expression(),
            Routes::domain_pattern().expression(),
            Routes::gateway_certificate_id().expression(),
        ]
    }
}

impl FromRow for RouteRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            gateway_scope_id: decode(row, 4)?,
            gateway_node_id: decode(row, 5)?,
            hostname: decode(row, 6)?,
            path_prefix: decode(row, 7)?,
            workload_id: decode(row, 8)?,
            workload_revision_id: decode(row, 9)?,
            runtime_unit_id: decode(row, 10)?,
            runtime_generation: decode(row, 11)?,
            port_name: decode(row, 12)?,
            upstream_origin: decode(row, 13)?,
            target_observed_at: decode(row, 14)?,
            state: decode(row, 15)?,
            gateway_revision: decode(row, 16)?,
            gateway_command_id: decode(row, 17)?,
            snapshot_digest: decode(row, 18)?,
            failure: decode(row, 19)?,
            aggregate_version: decode(row, 20)?,
            created_at: decode(row, 21)?,
            updated_at: decode(row, 22)?,
            activated_at: decode(row, 23)?,
            domain_claim_id: decode(row, 24)?,
            domain_pattern: decode(row, 25)?,
            gateway_certificate_id: decode(row, 26)?,
        })
    }
}

impl RouteRow {
    pub(in super::super) fn route(self) -> Result<Route, RepositoryError> {
        let workload_id = WorkloadId::from_uuid(self.workload_id);
        let target = RouteTarget::new(
            workload_id,
            WorkloadRevisionId::from_uuid(self.workload_revision_id),
            self.runtime_unit_id,
            self.runtime_generation,
            RoutePortName::parse(self.port_name).map_err(stored("port name"))?,
            UpstreamEndpoint::parse(self.upstream_origin).map_err(stored("upstream endpoint"))?,
            self.target_observed_at,
        )
        .map_err(stored("target"))?;
        let route = Route {
            id: RouteId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            gateway_scope_id: GatewayScopeId::from_uuid(self.gateway_scope_id),
            gateway_node_id: NodeId::from_uuid(self.gateway_node_id),
            hostname: RouteHostname::parse(self.hostname).map_err(stored("hostname"))?,
            path_prefix: RoutePath::parse(self.path_prefix).map_err(stored("path"))?,
            domain_claim_id: self.domain_claim_id.map(DomainClaimId::from_uuid),
            domain_pattern: self
                .domain_pattern
                .map(DomainNamePattern::parse)
                .transpose()
                .map_err(stored("domain pattern"))?,
            gateway_certificate_id: self
                .gateway_certificate_id
                .map(GatewayCertificateId::from_uuid),
            workload_id,
            target,
            state: RouteState::parse(&self.state).map_err(stored("state"))?,
            gateway_revision: Some(self.gateway_revision),
            gateway_command_id: Some(NodeCommandId::from_uuid(self.gateway_command_id)),
            snapshot_digest: Some(self.snapshot_digest),
            failure: self.failure,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            activated_at: self.activated_at,
        };
        validate_stored_route(&route)?;
        Ok(route)
    }
}

pub(in super::super) struct PublicationRow {
    node_id: Uuid,
    revision: u64,
    expected_revision: Option<u64>,
    command_id: Uuid,
    command_correlation_id: Uuid,
    snapshot_digest: String,
    acl: String,
    state: String,
    failure: Option<String>,
    command_issued_at: DateTime<Utc>,
    command_not_after: DateTime<Utc>,
    snapshot_expires_at: DateTime<Utc>,
    acknowledged_at: Option<DateTime<Utc>>,
    certificate_request: Option<serde_json::Value>,
}

pub(in super::super) struct PublicationSelection;

impl Selection for PublicationSelection {
    type Output = PublicationRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayPublications::node_id().expression(),
            GatewayPublications::revision().expression(),
            GatewayPublications::expected_revision().expression(),
            GatewayPublications::command_id().expression(),
            GatewayPublications::command_correlation_id().expression(),
            GatewayPublications::snapshot_digest().expression(),
            GatewayPublications::acl().expression(),
            GatewayPublications::state().expression(),
            GatewayPublications::failure().expression(),
            GatewayPublications::command_issued_at().expression(),
            GatewayPublications::command_not_after().expression(),
            GatewayPublications::snapshot_expires_at().expression(),
            GatewayPublications::acknowledged_at().expression(),
            GatewayPublications::certificate_request().expression(),
        ]
    }
}

impl FromRow for PublicationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            node_id: decode(row, 0)?,
            revision: decode(row, 1)?,
            expected_revision: decode(row, 2)?,
            command_id: decode(row, 3)?,
            command_correlation_id: decode(row, 4)?,
            snapshot_digest: decode(row, 5)?,
            acl: decode(row, 6)?,
            state: decode(row, 7)?,
            failure: decode(row, 8)?,
            command_issued_at: decode(row, 9)?,
            command_not_after: decode(row, 10)?,
            snapshot_expires_at: decode(row, 11)?,
            acknowledged_at: decode(row, 12)?,
            certificate_request: decode(row, 13)?,
        })
    }
}

impl PublicationRow {
    pub(in super::super) fn publication(self) -> Result<GatewayPublication, RepositoryError> {
        let certificate_request = self
            .certificate_request
            .map(serde_json::from_value::<GatewayCertificateRequest>)
            .transpose()
            .map_err(|error| stored("certificate request")(error.to_string()))?;
        let publication = GatewayPublication {
            node_id: NodeId::from_uuid(self.node_id),
            revision: self.revision,
            expected_revision: self.expected_revision,
            command_id: NodeCommandId::from_uuid(self.command_id),
            command_correlation_id: self.command_correlation_id,
            snapshot_digest: self.snapshot_digest,
            acl: self.acl,
            certificate_request,
            state: GatewayPublicationState::parse(&self.state).map_err(stored("state"))?,
            failure: self.failure,
            command_issued_at: self.command_issued_at,
            command_not_after: self.command_not_after,
            snapshot_expires_at: self.snapshot_expires_at,
            acknowledged_at: self.acknowledged_at,
        };
        publication.snapshot().map_err(stored("snapshot"))?;
        Ok(publication)
    }
}
