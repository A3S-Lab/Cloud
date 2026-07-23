use super::build_artifact::{validate_sha256, BuildArtifact, ValidatedOciBuildOutput};
use super::build_evidence::BuildEvidence;
use super::oci_publication::{OciPublicationTarget, PublishedOciArtifact};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildRunId, EnvironmentId, NodeCommandId, NodeId, OperationId,
    OrganizationId, ProjectId, SourceRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const BUILD_RUN_NAMESPACE: Uuid = Uuid::from_bytes([
    0x92, 0x3e, 0x7a, 0x65, 0x74, 0xc0, 0x4c, 0xf6, 0xb1, 0xe2, 0x8d, 0xe9, 0x4e, 0x4d, 0x59, 0x92,
]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildRunStatus {
    Queued,
    Preparing,
    Prepared,
    Scheduled,
    Running,
    Validating,
    Publishing,
    Attesting,
    Cancelling,
    CleanupPending,
    Succeeded,
    Failed,
    Cancelled,
}

impl BuildRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Preparing => "preparing",
            Self::Prepared => "prepared",
            Self::Scheduled => "scheduled",
            Self::Running => "running",
            Self::Validating => "validating",
            Self::Publishing => "publishing",
            Self::Attesting => "attesting",
            Self::Cancelling => "cancelling",
            Self::CleanupPending => "cleanup_pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "queued" => Ok(Self::Queued),
            "preparing" => Ok(Self::Preparing),
            "prepared" => Ok(Self::Prepared),
            "scheduled" => Ok(Self::Scheduled),
            "running" => Ok(Self::Running),
            "validating" => Ok(Self::Validating),
            "publishing" => Ok(Self::Publishing),
            "attesting" => Ok(Self::Attesting),
            "cancelling" => Ok(Self::Cancelling),
            "cleanup_pending" => Ok(Self::CleanupPending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unsupported build run status {value:?}")),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildRun {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: BuildRunId,
    pub source_revision_id: SourceRevisionId,
    pub attempt: u32,
    pub retry_of_build_run_id: Option<BuildRunId>,
    pub operation_id: OperationId,
    pub status: BuildRunStatus,
    pub source_content_digest: Option<String>,
    pub input_artifact: Option<BuildArtifact>,
    pub node_id: Option<NodeId>,
    pub command_id: Option<NodeCommandId>,
    pub cleanup_command_id: Option<NodeCommandId>,
    pub runtime_spec_digest: Option<String>,
    pub runtime_output_artifact: Option<BuildArtifact>,
    pub output: Option<ValidatedOciBuildOutput>,
    pub publication_target: Option<OciPublicationTarget>,
    pub published_artifact: Option<PublishedOciArtifact>,
    pub evidence_required: bool,
    pub evidence: Option<Box<BuildEvidence>>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl BuildRun {
    pub const RUNTIME_GENERATION: u64 = 1;

    pub fn id_for(source_revision_id: SourceRevisionId) -> BuildRunId {
        BuildRunId::from_uuid(Uuid::new_v5(
            &BUILD_RUN_NAMESPACE,
            source_revision_id.as_uuid().as_bytes(),
        ))
    }

    pub fn id_for_attempt(
        source_revision_id: SourceRevisionId,
        attempt: u32,
    ) -> Result<BuildRunId, String> {
        if attempt == 0 {
            return Err("build attempt must be positive".into());
        }
        if attempt == 1 {
            return Ok(Self::id_for(source_revision_id));
        }
        let mut identity = [0_u8; 20];
        identity[..16].copy_from_slice(source_revision_id.as_uuid().as_bytes());
        identity[16..].copy_from_slice(&attempt.to_be_bytes());
        Ok(BuildRunId::from_uuid(Uuid::new_v5(
            &BUILD_RUN_NAMESPACE,
            &identity,
        )))
    }

    pub fn runtime_unit_id_for(build_run_id: BuildRunId) -> String {
        format!("cloud-build-{build_run_id}")
    }

    pub fn runtime_unit_id(&self) -> String {
        Self::runtime_unit_id_for(self.id)
    }

    pub fn reserve(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        requested_at: DateTime<Utc>,
    ) -> Self {
        let requested_at = canonical_timestamp(requested_at);
        let id = Self::id_for(source_revision_id);
        Self {
            organization_id,
            project_id,
            environment_id,
            id,
            source_revision_id,
            attempt: 1,
            retry_of_build_run_id: None,
            operation_id: OperationId::from_uuid(id.as_uuid()),
            status: BuildRunStatus::Queued,
            source_content_digest: None,
            input_artifact: None,
            node_id: None,
            command_id: None,
            cleanup_command_id: None,
            runtime_spec_digest: None,
            runtime_output_artifact: None,
            output: None,
            publication_target: None,
            published_artifact: None,
            evidence_required: true,
            evidence: None,
            failure: None,
            aggregate_version: 1,
            requested_at,
            updated_at: requested_at,
            started_at: None,
            cancellation_requested_at: None,
            finished_at: None,
        }
    }

    pub fn retry(previous: &Self, requested_at: DateTime<Utc>) -> Result<Self, String> {
        previous.validate()?;
        if !matches!(
            previous.status,
            BuildRunStatus::Failed | BuildRunStatus::Cancelled
        ) {
            return Err("only a failed or cancelled build run can be retried".into());
        }
        let requested_at = canonical_timestamp(requested_at);
        if requested_at < previous.updated_at {
            return Err("build retry time regressed".into());
        }
        let attempt = previous
            .attempt
            .checked_add(1)
            .ok_or_else(|| "build attempt overflowed".to_owned())?;
        let id = Self::id_for_attempt(previous.source_revision_id, attempt)?;
        let retry = Self {
            organization_id: previous.organization_id,
            project_id: previous.project_id,
            environment_id: previous.environment_id,
            id,
            source_revision_id: previous.source_revision_id,
            attempt,
            retry_of_build_run_id: Some(previous.id),
            operation_id: OperationId::from_uuid(id.as_uuid()),
            status: BuildRunStatus::Queued,
            source_content_digest: None,
            input_artifact: None,
            node_id: None,
            command_id: None,
            cleanup_command_id: None,
            runtime_spec_digest: None,
            runtime_output_artifact: None,
            output: None,
            publication_target: None,
            published_artifact: None,
            evidence_required: true,
            evidence: None,
            failure: None,
            aggregate_version: 1,
            requested_at,
            updated_at: requested_at,
            started_at: None,
            cancellation_requested_at: None,
            finished_at: None,
        };
        retry.validate()?;
        Ok(retry)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.requested_at = canonical_timestamp(self.requested_at);
        self.updated_at = canonical_timestamp(self.updated_at);
        self.started_at = self.started_at.map(canonical_timestamp);
        self.cancellation_requested_at = self.cancellation_requested_at.map(canonical_timestamp);
        self.finished_at = self.finished_at.map(canonical_timestamp);
        self.validate()?;
        Ok(self)
    }

    pub fn begin_preparation(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        self.transition(BuildRunStatus::Queued, BuildRunStatus::Preparing, at)?;
        self.started_at.get_or_insert(self.updated_at);
        Ok(())
    }

    pub fn record_input(
        &mut self,
        source_content_digest: String,
        input_artifact: BuildArtifact,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        validate_sha256(&source_content_digest, "source content digest")?;
        input_artifact.validate()?;
        if self.status == BuildRunStatus::Prepared {
            return if self.source_content_digest.as_ref() == Some(&source_content_digest)
                && self.input_artifact.as_ref() == Some(&input_artifact)
            {
                self.observe_time(at)
            } else {
                Err("prepared build run cannot change its immutable input".into())
            };
        }
        self.transition(BuildRunStatus::Preparing, BuildRunStatus::Prepared, at)?;
        self.source_content_digest = Some(source_content_digest);
        self.input_artifact = Some(input_artifact);
        Ok(())
    }

    pub fn schedule(
        &mut self,
        node_id: NodeId,
        runtime_spec_digest: String,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        validate_sha256(&runtime_spec_digest, "Runtime specification digest")?;
        if self.status == BuildRunStatus::Scheduled {
            return if self.node_id == Some(node_id)
                && self.runtime_spec_digest.as_ref() == Some(&runtime_spec_digest)
            {
                self.observe_time(at)
            } else {
                Err("scheduled build run cannot change its node or Runtime specification".into())
            };
        }
        self.transition(BuildRunStatus::Prepared, BuildRunStatus::Scheduled, at)?;
        self.node_id = Some(node_id);
        self.runtime_spec_digest = Some(runtime_spec_digest);
        Ok(())
    }

    pub fn dispatch(&mut self, command_id: NodeCommandId, at: DateTime<Utc>) -> Result<(), String> {
        if self.status == BuildRunStatus::Running {
            return if self.command_id == Some(command_id) {
                self.observe_time(at)
            } else {
                Err("running build cannot change its Runtime command".into())
            };
        }
        if self.node_id.is_none() || self.runtime_spec_digest.is_none() {
            return Err("build run cannot dispatch before scheduling".into());
        }
        self.transition(BuildRunStatus::Scheduled, BuildRunStatus::Running, at)?;
        self.command_id = Some(command_id);
        Ok(())
    }

    pub fn begin_validation(
        &mut self,
        artifact: BuildArtifact,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        artifact.validate()?;
        if self.status == BuildRunStatus::Validating {
            return if self.runtime_output_artifact.as_ref() == Some(&artifact) {
                self.observe_time(at)
            } else {
                Err("validating build cannot change its Runtime output artifact".into())
            };
        }
        self.transition(BuildRunStatus::Running, BuildRunStatus::Validating, at)?;
        self.runtime_output_artifact = Some(artifact);
        Ok(())
    }

    pub fn record_validated_output(
        &mut self,
        output: ValidatedOciBuildOutput,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        output.validate()?;
        if self.status != BuildRunStatus::Validating {
            return Err("validated output requires a validating build run".into());
        }
        if self.runtime_output_artifact.as_ref() != Some(&output.artifact) {
            return Err("validated output changed the Runtime output artifact".into());
        }
        let at = self.canonical_time(at)?;
        if let Some(existing) = &self.output {
            if existing != &output {
                return Err("validated build output cannot change".into());
            }
            return self.observe_time(at);
        }
        self.output = Some(output);
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn begin_publication(
        &mut self,
        target: OciPublicationTarget,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        target.validate()?;
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| "OCI publication requires validated output".to_owned())?;
        if !target.matches_output(output) {
            return Err("OCI publication target changed the validated output descriptor".into());
        }
        if self.status == BuildRunStatus::Publishing {
            return if self.publication_target.as_ref() == Some(&target) {
                self.observe_time(at)
            } else {
                Err("publishing build cannot change its immutable target".into())
            };
        }
        self.transition(BuildRunStatus::Validating, BuildRunStatus::Publishing, at)?;
        self.publication_target = Some(target);
        Ok(())
    }

    pub fn record_published_artifact(
        &mut self,
        artifact: PublishedOciArtifact,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        artifact.validate()?;
        if !matches!(
            self.status,
            BuildRunStatus::Publishing | BuildRunStatus::Cancelling
        ) {
            return Err("published OCI artifact requires a publishing or cancelling build".into());
        }
        let target = self
            .publication_target
            .as_ref()
            .ok_or_else(|| "published OCI artifact is missing its durable target".to_owned())?;
        if !artifact.matches_target(target) {
            return Err("published OCI artifact changed its durable target".into());
        }
        let at = self.canonical_time(at)?;
        if let Some(existing) = &self.published_artifact {
            if existing != &artifact {
                return Err("published OCI artifact cannot change".into());
            }
            return self.observe_time(at);
        }
        self.published_artifact = Some(artifact);
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn begin_attestation(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        if !self.evidence_required {
            return Err("build run does not require supply-chain evidence".into());
        }
        if self.published_artifact.is_none() {
            return Err("build attestation requires a published OCI artifact".into());
        }
        if self.evidence.is_some() {
            return self.observe_time(at);
        }
        if self.status == BuildRunStatus::Attesting {
            return self.observe_time(at);
        }
        if !matches!(
            self.status,
            BuildRunStatus::Publishing | BuildRunStatus::Cancelling
        ) {
            return Err(format!(
                "build run cannot attest from {}",
                self.status.as_str()
            ));
        }
        let at = self.canonical_time(at)?;
        self.status = BuildRunStatus::Attesting;
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn record_evidence(
        &mut self,
        evidence: BuildEvidence,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        evidence.validate()?;
        self.validate_evidence_binding(&evidence)?;
        if !matches!(
            self.status,
            BuildRunStatus::Attesting | BuildRunStatus::Cancelling
        ) {
            return Err("build evidence requires an attesting or cancelling build".into());
        }
        let at = self.canonical_time(at)?;
        if let Some(existing) = &self.evidence {
            if existing.as_ref() != &evidence {
                return Err("verified build evidence cannot change".into());
            }
            return self.observe_time(at);
        }
        self.evidence = Some(Box::new(evidence));
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn record_failure(&mut self, reason: String, at: DateTime<Utc>) -> Result<(), String> {
        validate_reason(&reason)?;
        if self.status.is_terminal() {
            return Err("terminal build run cannot fail".into());
        }
        if self.cancellation_requested_at.is_some()
            && !(self.evidence_required
                && self.published_artifact.is_some()
                && self.evidence.is_none())
        {
            return Err(
                "cancelling build can fail only when required published evidence is missing".into(),
            );
        }
        let at = self.canonical_time(at)?;
        if let Some(existing) = &self.failure {
            if existing != &reason {
                return Err("build failure reason cannot change".into());
            }
            return self.observe_time(at);
        }
        self.failure = Some(reason);
        self.status = BuildRunStatus::CleanupPending;
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn request_cancellation(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        if self.status.is_terminal() {
            return Err("terminal build run cannot request cancellation".into());
        }
        let at = self.canonical_time(at)?;
        if self.cancellation_requested_at.is_some() {
            return Ok(());
        }
        self.cancellation_requested_at = Some(at);
        self.status = BuildRunStatus::Cancelling;
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn begin_cleanup(
        &mut self,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.command_id.is_none() || self.node_id.is_none() {
            return Err("Runtime cleanup requires a dispatched build Task".into());
        }
        if !matches!(
            self.status,
            BuildRunStatus::Publishing
                | BuildRunStatus::Attesting
                | BuildRunStatus::Cancelling
                | BuildRunStatus::CleanupPending
        ) {
            return Err("build run is not ready for Runtime cleanup".into());
        }
        if self.status == BuildRunStatus::Publishing && self.published_artifact.is_none() {
            return Err("successful Runtime cleanup requires a published OCI artifact".into());
        }
        if self.published_artifact.is_some()
            && self.evidence_required
            && self.evidence.is_none()
            && self.failure.is_none()
        {
            return Err("published OCI artifact must have verified evidence before cleanup".into());
        }
        let at = self.canonical_time(at)?;
        match self.cleanup_command_id {
            Some(existing) if existing != command_id => {
                return Err("build cleanup command cannot change without an explicit retry".into());
            }
            Some(_) if self.status == BuildRunStatus::CleanupPending => return Ok(()),
            Some(_) => {}
            None => self.cleanup_command_id = Some(command_id),
        }
        self.status = BuildRunStatus::CleanupPending;
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn retry_cleanup(
        &mut self,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != BuildRunStatus::CleanupPending
            || self.command_id.is_none()
            || self.node_id.is_none()
            || self.cleanup_command_id.is_none()
        {
            return Err("build run is not ready to retry Runtime cleanup".into());
        }
        if self.cleanup_command_id == Some(command_id) {
            return self.observe_time(at);
        }
        let at = self.canonical_time(at)?;
        self.cleanup_command_id = Some(command_id);
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    pub fn complete(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        if self.status.is_terminal() {
            return self.observe_time(at);
        }
        let at = self.canonical_time(at)?;
        if self.command_id.is_some() && self.cleanup_command_id.is_none() {
            return Err("dispatched build Task must be removed before completion".into());
        }
        self.status = if self.cancellation_requested_at.is_some() {
            BuildRunStatus::Cancelled
        } else if self.failure.is_some() {
            BuildRunStatus::Failed
        } else if self.published_artifact.is_some() {
            BuildRunStatus::Succeeded
        } else {
            return Err("build completion has no terminal outcome".into());
        };
        self.finished_at = Some(at);
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        let expected_id = Self::id_for_attempt(self.source_revision_id, self.attempt)?;
        let expected_retry = if self.attempt == 1 {
            None
        } else {
            Some(Self::id_for_attempt(
                self.source_revision_id,
                self.attempt - 1,
            )?)
        };
        if self.aggregate_version == 0
            || self.id != expected_id
            || self.retry_of_build_run_id != expected_retry
            || self.operation_id.as_uuid() != self.id.as_uuid()
            || self.updated_at < self.requested_at
            || self.started_at.is_some_and(|at| at < self.requested_at)
            || self
                .cancellation_requested_at
                .is_some_and(|at| at < self.requested_at)
            || self.finished_at.is_some_and(|at| at < self.requested_at)
            || self.status.is_terminal() != self.finished_at.is_some()
        {
            return Err("stored build run identity, version, or timestamps are invalid".into());
        }
        match (&self.source_content_digest, &self.input_artifact) {
            (Some(digest), Some(artifact)) => {
                validate_sha256(digest, "source content digest")?;
                artifact.validate()?;
            }
            (None, None) => {}
            _ => return Err("build input digest and artifact must be stored together".into()),
        }
        if self.node_id.is_some() != self.runtime_spec_digest.is_some()
            || self.input_artifact.is_none() && self.node_id.is_some()
            || self.node_id.is_none() && self.command_id.is_some()
            || self.command_id.is_none() && self.cleanup_command_id.is_some()
            || self.command_id.is_none() && self.runtime_output_artifact.is_some()
            || self.input_artifact.is_some() && self.started_at.is_none()
        {
            return Err("stored build run has an incomplete Runtime execution chain".into());
        }
        if let Some(digest) = &self.runtime_spec_digest {
            validate_sha256(digest, "Runtime specification digest")?;
        }
        if let Some(output) = &self.output {
            output.validate()?;
            if self.runtime_output_artifact.as_ref() != Some(&output.artifact) {
                return Err("validated output changed the Runtime output artifact".into());
            }
        }
        if let Some(artifact) = &self.runtime_output_artifact {
            artifact.validate()?;
        }
        match (&self.publication_target, &self.published_artifact) {
            (Some(target), published) => {
                target.validate()?;
                let output = self.output.as_ref().ok_or_else(|| {
                    "stored OCI publication target is missing validated output".to_owned()
                })?;
                if !target.matches_output(output) {
                    return Err("stored OCI publication target changed the validated output".into());
                }
                if let Some(artifact) = published {
                    artifact.validate()?;
                    if !artifact.matches_target(target) {
                        return Err("stored published OCI artifact changed its target".into());
                    }
                }
            }
            (None, None) => {}
            (None, Some(_)) => {
                return Err("stored published OCI artifact is missing its target".into())
            }
        }
        if let Some(reason) = &self.failure {
            validate_reason(reason)?;
        }
        match &self.evidence {
            Some(evidence) => {
                if !self.evidence_required {
                    return Err("stored build evidence is not required by this build run".into());
                }
                evidence.validate()?;
                self.validate_evidence_binding(evidence)?;
            }
            None if self.published_artifact.is_some()
                && self.evidence_required
                && self.cleanup_command_id.is_some()
                && self.failure.is_none() =>
            {
                return Err(
                    "stored published build entered cleanup without verified evidence".into(),
                )
            }
            None => {}
        }
        if self.status == BuildRunStatus::Succeeded
            && (self.published_artifact.is_none()
                || self.evidence_required && self.evidence.is_none()
                || self.failure.is_some()
                || self.cancellation_requested_at.is_some()
                || self.cleanup_command_id.is_none())
        {
            return Err("successful build run has an inconsistent terminal outcome".into());
        }
        if self.status == BuildRunStatus::Failed
            && (self.failure.is_none() || self.cancellation_requested_at.is_some())
        {
            return Err("failed build run is missing its failure reason".into());
        }
        if self.status == BuildRunStatus::Cancelled
            && (self.cancellation_requested_at.is_none()
                || self.published_artifact.is_some()
                    && self.evidence_required
                    && self.evidence.is_none()
                    && self.failure.is_none())
        {
            return Err("cancelled published build is missing its request or evidence".into());
        }
        if self.command_id.is_some()
            && self.status.is_terminal()
            && self.cleanup_command_id.is_none()
        {
            return Err("terminal build run retained its dispatched Runtime Task".into());
        }
        match self.status {
            BuildRunStatus::Queued
                if self.started_at.is_some()
                    || self.input_artifact.is_some()
                    || self.node_id.is_some()
                    || self.command_id.is_some()
                    || self.runtime_output_artifact.is_some()
                    || self.output.is_some()
                    || self.publication_target.is_some()
                    || self.published_artifact.is_some()
                    || self.evidence.is_some()
                    || self.failure.is_some()
                    || self.cancellation_requested_at.is_some()
                    || self.cleanup_command_id.is_some() =>
            {
                return Err("queued build run contains execution state".into());
            }
            BuildRunStatus::Preparing
                if self.started_at.is_none()
                    || self.input_artifact.is_some()
                    || self.node_id.is_some()
                    || self.publication_target.is_some()
                    || self.published_artifact.is_some()
                    || self.evidence.is_some()
                    || self.failure.is_some()
                    || self.cancellation_requested_at.is_some() =>
            {
                return Err("preparing build run has inconsistent execution state".into());
            }
            BuildRunStatus::Prepared
                if self.started_at.is_none()
                    || self.input_artifact.is_none()
                    || self.node_id.is_some()
                    || self.publication_target.is_some()
                    || self.published_artifact.is_some()
                    || self.evidence.is_some()
                    || self.failure.is_some()
                    || self.cancellation_requested_at.is_some() =>
            {
                return Err("prepared build run has inconsistent execution state".into());
            }
            BuildRunStatus::Scheduled
                if self.input_artifact.is_none()
                    || self.node_id.is_none()
                    || self.command_id.is_some()
                    || self.publication_target.is_some()
                    || self.published_artifact.is_some()
                    || self.evidence.is_some()
                    || self.failure.is_some()
                    || self.cancellation_requested_at.is_some() =>
            {
                return Err("scheduled build run has inconsistent execution state".into());
            }
            BuildRunStatus::Running
                if self.command_id.is_none()
                    || self.runtime_output_artifact.is_some()
                    || self.publication_target.is_some()
                    || self.published_artifact.is_some()
                    || self.evidence.is_some()
                    || self.failure.is_some()
                    || self.cancellation_requested_at.is_some() =>
            {
                return Err("running build run has inconsistent execution state".into());
            }
            BuildRunStatus::Validating
                if self.command_id.is_none()
                    || self.runtime_output_artifact.is_none()
                    || self.cleanup_command_id.is_some()
                    || self.publication_target.is_some()
                    || self.published_artifact.is_some()
                    || self.evidence.is_some()
                    || self.failure.is_some()
                    || self.cancellation_requested_at.is_some() =>
            {
                return Err("validating build run has inconsistent execution state".into());
            }
            BuildRunStatus::Publishing
                if self.command_id.is_none()
                    || self.runtime_output_artifact.is_none()
                    || self.output.is_none()
                    || self.publication_target.is_none()
                    || self.cleanup_command_id.is_some()
                    || self.evidence.is_some()
                    || self.failure.is_some()
                    || self.cancellation_requested_at.is_some() =>
            {
                return Err("publishing build run has inconsistent execution state".into());
            }
            BuildRunStatus::Attesting
                if self.command_id.is_none()
                    || self.output.is_none()
                    || self.publication_target.is_none()
                    || self.published_artifact.is_none()
                    || !self.evidence_required
                    || self.cleanup_command_id.is_some()
                    || self.failure.is_some() =>
            {
                return Err("attesting build run has inconsistent execution state".into());
            }
            BuildRunStatus::Cancelling if self.cancellation_requested_at.is_none() => {
                return Err("cancelling build run has no cancellation request".into());
            }
            BuildRunStatus::CleanupPending
                if self.published_artifact.is_none()
                    && self.failure.is_none()
                    && self.cancellation_requested_at.is_none() =>
            {
                return Err("cleanup-pending build run has no terminal intent".into());
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_evidence_binding(&self, evidence: &BuildEvidence) -> Result<(), String> {
        if !self.evidence_required
            || evidence.build_run_id != self.id
            || evidence.operation_id != self.operation_id
            || evidence.source_revision_id != self.source_revision_id
            || evidence.attempt != self.attempt
            || Some(evidence.source_content_digest.as_str())
                != self.source_content_digest.as_deref()
            || Some(evidence.runtime_spec_digest.as_str()) != self.runtime_spec_digest.as_deref()
            || Some(&evidence.artifact) != self.published_artifact.as_ref()
            || self
                .output
                .as_ref()
                .is_none_or(|output| output.platforms != evidence.platforms)
        {
            return Err("build evidence changed its durable BuildRun binding".into());
        }
        Ok(())
    }

    fn transition(
        &mut self,
        expected: BuildRunStatus,
        next: BuildRunStatus,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if self.status == next {
            return self.observe_time(at);
        }
        if self.status != expected {
            return Err(format!(
                "build run cannot transition from {} to {}",
                self.status.as_str(),
                next.as_str()
            ));
        }
        self.status = next;
        self.aggregate_version += 1;
        self.updated_at = at;
        Ok(())
    }

    fn observe_time(&self, at: DateTime<Utc>) -> Result<(), String> {
        self.canonical_time(at).map(|_| ())
    }

    fn canonical_time(&self, at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        let at = canonical_timestamp(at);
        if at < self.updated_at {
            return Err("build run update time regressed".into());
        }
        Ok(at)
    }
}

fn validate_reason(reason: &str) -> Result<(), String> {
    if reason.trim().is_empty() || reason.len() > 16 * 1024 || reason.contains(['\0', '\r', '\n']) {
        return Err("build failure reason is invalid".into());
    }
    Ok(())
}
