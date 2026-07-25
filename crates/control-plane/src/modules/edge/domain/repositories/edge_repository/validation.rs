use super::*;

impl StageGatewayRolloutRollback {
    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        self.failed_rollout.validate()?;
        self.rollback.validate()?;
        self.rollout.validate()?;
        let desired_replicas = u32::try_from(self.scope.member_node_ids.len())
            .map_err(|_| "Gateway rollback member count exceeds supported bounds".to_string())?;
        if self.failed_rollout.id != self.rollback.failed_rollout_id
            || self.failed_rollout.gateway_scope_id != self.scope.id
            || self.failed_rollout.membership_generation != self.scope.membership_generation
            || self.failed_rollout.generation != self.rollback.failed_generation
            || self.rollback.gateway_scope_id != self.scope.id
            || self.rollback.membership_generation != self.scope.membership_generation
            || self.rollback.state
                != crate::modules::edge::domain::GatewayRolloutRollbackState::Staged
            || self.expected_rollback_version.checked_add(1)
                != Some(self.rollback.aggregate_version)
            || self.rollout.id != self.rollback.rollback_rollout_id
            || self.rollout.generation != self.rollback.rollback_generation
            || self.rollout.gateway_scope_id != self.scope.id
            || self.rollout.membership_generation != self.scope.membership_generation
            || self.rollout.correlation_id != self.rollback.rollback_rollout_id.as_uuid()
            || self.rollout.policy.min_ready != desired_replicas
            || self.rollout.policy.max_unavailable != 0
            || self.rollout.state != GatewayRolloutState::Pending
            || self.rollout.aggregate_version != 1
            || self.event.event_key != "edge.gateway-rollout.staged"
            || self.event.schema_version != 1
            || self.event.organization_id != self.scope.organization_id.as_uuid()
            || self.event.aggregate_id != self.rollout.id.as_uuid()
            || self.event.aggregate_version != self.rollout.aggregate_version
            || self.event.occurred_at != self.rollout.started_at
            || self.event.correlation_id != self.rollout.correlation_id
        {
            return Err("Gateway rollout rollback stage bundle is inconsistent".into());
        }
        let mut publications = self.publications.iter().collect::<Vec<_>>();
        publications.sort_by_key(|publication| publication.node_id);
        if publications.len() != self.rollout.replicas.len()
            || publications
                .iter()
                .zip(&self.rollout.replicas)
                .any(|(publication, replica)| {
                    publication.node_id != replica.node_id
                        || publication.revision != replica.revision
                        || publication.command_id != replica.command_id
                        || publication.command_correlation_id != self.rollout.correlation_id
                        || publication.snapshot_digest != replica.snapshot_digest
                        || publication.snapshot_expires_at != replica.snapshot_expires_at
                        || publication.state != GatewayPublicationState::Pending
                        || publication.failure.is_some()
                        || publication.acknowledged_at.is_some()
                        || publication
                            .certificate_request
                            .as_ref()
                            .map(|request| GatewayCertificateId::from_uuid(request.certificate_id))
                            != replica.gateway_certificate_id
                })
        {
            return Err(
                "Gateway rollback publications do not match exact member projections".into(),
            );
        }
        let expected_nodes = self
            .scope
            .member_node_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if self
            .expected_scope_versions
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            != expected_nodes
        {
            return Err("Gateway rollback scope versions do not cover exact membership".into());
        }
        let mut certificate_ids = std::collections::BTreeSet::new();
        let mut certificate_nodes = std::collections::BTreeSet::new();
        for certificate in &self.certificates {
            if !certificate_ids.insert(certificate.id)
                || !certificate_nodes.insert(certificate.node_id)
                || certificate.organization_id != self.scope.organization_id
                || certificate.state != GatewayCertificateState::Provisioning
            {
                return Err("new Gateway rollback certificate set is inconsistent".into());
            }
            let publication = publications
                .iter()
                .copied()
                .find(|publication| publication.node_id == certificate.node_id)
                .ok_or_else(|| {
                    "new Gateway rollback certificate omitted its publication".to_string()
                })?;
            if certificate.gateway_revision != publication.revision
                || certificate.gateway_command_id != publication.command_id
                || certificate.snapshot_digest != publication.snapshot_digest
                || publication.certificate_request.as_ref() != Some(&certificate.request)
            {
                return Err("new Gateway rollback certificate binding is inconsistent".into());
            }
        }
        for certificate in &self.reused_certificates {
            if !certificate_ids.insert(certificate.id)
                || !certificate_nodes.insert(certificate.node_id)
                || certificate.organization_id != self.scope.organization_id
                || certificate.state != GatewayCertificateState::Ready
                || certificate.material.as_ref().is_none_or(|material| {
                    material.issued_at > self.rollout.started_at
                        || material.expires_at <= self.rollout.started_at
                })
            {
                return Err("reused Gateway rollback certificate set is inconsistent".into());
            }
            let publication = publications
                .iter()
                .copied()
                .find(|publication| publication.node_id == certificate.node_id)
                .ok_or_else(|| {
                    "reused Gateway rollback certificate omitted its publication".to_string()
                })?;
            if publication.certificate_request.as_ref() != Some(&certificate.request) {
                return Err("reused Gateway rollback certificate request changed".into());
            }
        }
        let publication_certificate_nodes = publications
            .iter()
            .filter(|publication| publication.certificate_request.is_some())
            .map(|publication| publication.node_id)
            .collect::<std::collections::BTreeSet<_>>();
        if publication_certificate_nodes != certificate_nodes {
            return Err(
                "Gateway rollback certificate evidence does not cover every TLS snapshot".into(),
            );
        }
        for publication in &self.publications {
            publication.snapshot()?;
        }
        Ok(())
    }
}

impl StageGatewayRollout {
    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        self.rollout.validate()?;
        if self.rollout.gateway_scope_id != self.scope.id
            || self.rollout.membership_generation != self.scope.membership_generation
            || self.rollout.policy != self.scope.rollout_policy
            || self.rollout.state != crate::modules::edge::domain::GatewayRolloutState::Pending
            || self.rollout.aggregate_version != 1
            || self.event.event_key != "edge.gateway-rollout.staged"
            || self.event.schema_version != 1
            || self.event.organization_id != self.scope.organization_id.as_uuid()
            || self.event.aggregate_id != self.rollout.id.as_uuid()
            || self.event.aggregate_version != self.rollout.aggregate_version
            || self.event.occurred_at != self.rollout.started_at
            || self.event.correlation_id != self.rollout.correlation_id
        {
            return Err("Gateway rollout stage bundle is inconsistent".into());
        }
        let mut publications = self.publications.iter().collect::<Vec<_>>();
        publications.sort_by_key(|publication| publication.node_id);
        if publications.len() != self.rollout.replicas.len()
            || publications
                .iter()
                .zip(&self.rollout.replicas)
                .any(|(publication, replica)| {
                    publication.node_id != replica.node_id
                        || publication.revision != replica.revision
                        || publication.command_id != replica.command_id
                        || publication.snapshot_digest != replica.snapshot_digest
                        || publication.snapshot_expires_at != replica.snapshot_expires_at
                        || publication
                            .certificate_request
                            .as_ref()
                            .map(|request| GatewayCertificateId::from_uuid(request.certificate_id))
                            != replica.gateway_certificate_id
                })
        {
            return Err("Gateway rollout publications do not match its replica projection".into());
        }
        let expected_nodes = self
            .rollout
            .replicas
            .iter()
            .map(|replica| replica.node_id)
            .collect::<std::collections::BTreeSet<_>>();
        if self
            .expected_scope_versions
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            != expected_nodes
        {
            return Err(
                "Gateway rollout scope versions do not cover the exact desired membership".into(),
            );
        }
        let mut certificates = self.certificates.iter().collect::<Vec<_>>();
        certificates.sort_by_key(|certificate| certificate.node_id);
        let expected_certificates = publications
            .iter()
            .filter_map(|publication| {
                publication
                    .certificate_request
                    .as_ref()
                    .map(|request| (publication, request.certificate_id))
            })
            .collect::<Vec<_>>();
        if certificates.len() != expected_certificates.len()
            || certificates.iter().zip(expected_certificates).any(
                |(certificate, (publication, certificate_id))| {
                    certificate.id.as_uuid() != certificate_id
                        || certificate.organization_id != self.scope.organization_id
                        || certificate.node_id != publication.node_id
                        || certificate.gateway_revision != publication.revision
                        || certificate.gateway_command_id != publication.command_id
                        || certificate.snapshot_digest != publication.snapshot_digest
                        || certificate.state
                            != crate::modules::edge::domain::GatewayCertificateState::Provisioning
                        || publication.certificate_request.as_ref() != Some(&certificate.request)
                },
            )
        {
            return Err("Gateway rollout certificate projection is inconsistent".into());
        }
        for publication in &self.publications {
            publication.snapshot()?;
        }
        self.validate_route_replicas(&publications, &certificates)?;
        Ok(())
    }

    fn validate_route_replicas(
        &self,
        publications: &[&GatewayPublication],
        certificates: &[&GatewayCertificate],
    ) -> Result<(), String> {
        if self.route_replicas.is_empty() {
            return if self.route_event.is_none() {
                Ok(())
            } else {
                Err("Gateway rollout without Route projections has a Route event".into())
            };
        }
        let route_event = self
            .route_event
            .as_ref()
            .ok_or_else(|| "Gateway Route rollout requires its logical Route event".to_string())?;
        let mut routes = self.route_replicas.iter().collect::<Vec<_>>();
        routes.sort_by_key(|route| route.gateway_node_id);
        if routes.len() != publications.len()
            || routes
                .windows(2)
                .any(|routes| routes[0].gateway_node_id == routes[1].gateway_node_id)
        {
            return Err(
                "Gateway Route rollout must project every physical member exactly once".into(),
            );
        }
        let primary = routes
            .iter()
            .copied()
            .find(|route| route.gateway_node_id == self.scope.node_id)
            .ok_or_else(|| {
                "Gateway Route rollout omitted its bootstrap primary projection".to_string()
            })?;
        if route_event.event_key != "edge.route.publication-staged"
            || route_event.schema_version != 3
            || route_event.organization_id != self.scope.organization_id.as_uuid()
            || route_event.aggregate_id != primary.id.as_uuid()
            || route_event.aggregate_version != primary.aggregate_version
            || route_event.occurred_at != primary.updated_at
            || route_event.correlation_id != self.rollout.correlation_id
        {
            return Err("Gateway Route rollout event is inconsistent".into());
        }
        for (route, publication) in routes.iter().zip(publications) {
            route.validate_target_binding()?;
            let certificate = route
                .gateway_certificate_id
                .and_then(|certificate_id| {
                    certificates
                        .iter()
                        .copied()
                        .find(|certificate| certificate.id == certificate_id)
                })
                .ok_or_else(|| {
                    "Gateway Route rollout projection omitted its certificate".to_string()
                })?;
            if route.id != primary.id
                || route.organization_id != primary.organization_id
                || route.project_id != primary.project_id
                || route.environment_id != primary.environment_id
                || route.gateway_scope_id != self.scope.id
                || route.hostname != primary.hostname
                || route.path_prefix != primary.path_prefix
                || route.domain_claim_id != primary.domain_claim_id
                || route.domain_pattern != primary.domain_pattern
                || route.workload_id != primary.workload_id
                || route.target.workload_revision_id != primary.target.workload_revision_id
                || route.target.runtime_unit_id != primary.target.runtime_unit_id
                || route.target.runtime_generation != primary.target.runtime_generation
                || route.target.port_name != primary.target.port_name
                || route.state != RouteState::Publishing
                || route.gateway_node_id != publication.node_id
                || route.gateway_revision != Some(publication.revision)
                || route.gateway_command_id != Some(publication.command_id)
                || route.snapshot_digest.as_deref() != Some(&publication.snapshot_digest)
                || route.failure.is_some()
                || route.aggregate_version != 2
                || route.created_at != self.rollout.started_at
                || route.updated_at != self.rollout.started_at
                || route.activated_at.is_some()
                || certificate.node_id != route.gateway_node_id
                || certificate.gateway_revision != publication.revision
                || certificate.gateway_command_id != publication.command_id
                || certificate.snapshot_digest != publication.snapshot_digest
            {
                return Err("Gateway Route rollout projection is inconsistent".into());
            }
        }
        Ok(())
    }
}

impl StageGatewayCertificateConvergence {
    pub fn validate(&self) -> Result<(), String> {
        self.convergence.validate()?;
        let convergence = &self.convergence;
        let publication = &self.publication;
        if convergence.state != GatewayCertificateConvergenceState::Pending
            || publication.state != crate::modules::edge::domain::GatewayPublicationState::Pending
            || convergence.node_id != publication.node_id
            || convergence.gateway_revision != publication.revision
            || convergence.gateway_command_id != publication.command_id
            || convergence.snapshot_digest != publication.snapshot_digest
            || publication.expected_revision.is_none()
            || self.event.organization_id != convergence.organization_id.as_uuid()
            || self.event.aggregate_id
                != convergence
                    .replacement_certificate_id
                    .unwrap_or(convergence.previous_certificate_id)
                    .as_uuid()
            || self.event.correlation_id != publication.command_correlation_id
        {
            return Err(
                "Gateway certificate convergence and complete publication are inconsistent".into(),
            );
        }
        match (
            convergence.replacement_certificate_id,
            publication.certificate_request.as_ref(),
            self.certificate.as_ref(),
        ) {
            (Some(certificate_id), Some(request), Some(certificate))
                if request.certificate_id == certificate_id.as_uuid()
                    && certificate.id == certificate_id
                    && certificate.organization_id == convergence.organization_id
                    && certificate.node_id == convergence.node_id
                    && certificate.gateway_revision == convergence.gateway_revision
                    && certificate.gateway_command_id == convergence.gateway_command_id
                    && certificate.snapshot_digest == convergence.snapshot_digest
                    && certificate.request == *request
                    && certificate.state
                        == crate::modules::edge::domain::GatewayCertificateState::Provisioning
                    && certificate.csr_digest.is_none()
                    && certificate.material.is_none() => {}
            (None, None, None) => {}
            _ => {
                return Err(
                    "Gateway certificate convergence replacement material is inconsistent".into(),
                )
            }
        }
        publication.snapshot()?;
        Ok(())
    }
}

impl StageGatewayRouteCutover {
    pub fn validate(&self) -> Result<(), String> {
        self.cutover.validate()?;
        let cutover = &self.cutover;
        let certificate = &self.certificate;
        let publication = &self.publication;
        if cutover.state != GatewayRouteCutoverState::Pending
            || publication.state != crate::modules::edge::domain::GatewayPublicationState::Pending
            || cutover.node_id != publication.node_id
            || cutover.gateway_revision != publication.revision
            || cutover.gateway_command_id != publication.command_id
            || cutover.snapshot_digest != publication.snapshot_digest
            || cutover.gateway_certificate_id != certificate.id
            || certificate.organization_id != cutover.organization_id
            || certificate.node_id != cutover.node_id
            || certificate.gateway_revision != cutover.gateway_revision
            || certificate.gateway_command_id != cutover.gateway_command_id
            || certificate.snapshot_digest != cutover.snapshot_digest
            || publication.certificate_request.as_ref() != Some(&certificate.request)
            || certificate.state
                != crate::modules::edge::domain::GatewayCertificateState::Provisioning
            || certificate.csr_digest.is_some()
            || certificate.material.is_some()
            || self.event.organization_id != cutover.organization_id.as_uuid()
            || self.event.aggregate_id != cutover.deployment_id.as_uuid()
            || self.event.correlation_id != publication.command_correlation_id
        {
            return Err("route cutover and complete Gateway publication are inconsistent".into());
        }
        publication.snapshot()?;
        Ok(())
    }
}

impl StageRoutePublication {
    pub fn validate(&self) -> Result<(), String> {
        let route = &self.route;
        let gateway_scope = &self.gateway_scope;
        let certificate = &self.certificate;
        let publication = &self.publication;
        if route.state != crate::modules::edge::domain::RouteState::Publishing
            || route.gateway_scope_id != gateway_scope.id
            || !gateway_scope.owns(
                route.organization_id,
                route.project_id,
                route.environment_id,
                route.gateway_node_id,
            )
            || route.gateway_node_id != publication.node_id
            || route.gateway_revision != Some(publication.revision)
            || route.gateway_command_id != Some(publication.command_id)
            || route.snapshot_digest.as_deref() != Some(&publication.snapshot_digest)
            || publication.state != crate::modules::edge::domain::GatewayPublicationState::Pending
            || route.gateway_certificate_id != Some(certificate.id)
            || certificate.node_id != publication.node_id
            || certificate.gateway_revision != publication.revision
            || certificate.gateway_command_id != publication.command_id
            || certificate.snapshot_digest != publication.snapshot_digest
            || publication.certificate_request.as_ref() != Some(&certificate.request)
            || certificate.state
                != crate::modules::edge::domain::GatewayCertificateState::Provisioning
            || certificate.csr_digest.is_some()
            || certificate.material.is_some()
            || route
                .domain_claim_id
                .is_none_or(|claim_id| !certificate.domain_claim_ids.contains(&claim_id))
            || self.event.correlation_id != publication.command_correlation_id
        {
            return Err("route and complete Gateway publication are inconsistent".into());
        }
        publication.snapshot()?;
        Ok(())
    }
}
