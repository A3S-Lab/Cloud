use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CrashBoundary {
    Publication,
    Evidence,
}

impl CrashBoundary {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Publication => "publication",
            Self::Evidence => "evidence",
        }
    }

    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "publication" => Ok(Self::Publication),
            "evidence" => Ok(Self::Evidence),
            _ => Err("G0 crash probe boundary is invalid".into()),
        }
    }
}

pub(super) struct ProbeEnvironment {
    pub(super) postgres_url: String,
    pub(super) paths: ProbePaths,
    pub(super) organization_id: OrganizationId,
    pub(super) build_id: BuildRunId,
    pub(super) boundary: CrashBoundary,
}

impl ProbeEnvironment {
    pub(super) fn read() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            postgres_url: required_environment(POSTGRES_ENV)?,
            paths: ProbePaths::new(Path::new(&required_environment(PROBE_ROOT_ENV)?)),
            organization_id: OrganizationId::from_uuid(Uuid::parse_str(&required_environment(
                PROBE_ORGANIZATION_ENV,
            )?)?),
            build_id: BuildRunId::from_uuid(Uuid::parse_str(&required_environment(
                PROBE_BUILD_ENV,
            )?)?),
            boundary: CrashBoundary::parse(&required_environment(PROBE_BOUNDARY_ENV)?)?,
        })
    }
}

pub(super) struct ProbePaths {
    pub(super) root: PathBuf,
    pub(super) artifact_store: PathBuf,
    pub(super) validation_root: PathBuf,
}

impl ProbePaths {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            artifact_store: root.join("control-plane-artifacts"),
            validation_root: root.join("g0-process-death-validation"),
        }
    }

    pub(super) fn publication_marker(&self) -> PathBuf {
        self.root.join(PUBLICATION_MARKER)
    }

    pub(super) fn evidence_marker(&self) -> PathBuf {
        self.root.join(EVIDENCE_MARKER)
    }
}

pub(super) async fn crash_at_boundary(
    boundary: CrashBoundary,
    paths: &ProbePaths,
    postgres_url: &str,
    organization_id: OrganizationId,
    build_id: BuildRunId,
) -> Result<ExitStatus, Box<dyn Error>> {
    let marker = match boundary {
        CrashBoundary::Publication => paths.publication_marker(),
        CrashBoundary::Evidence => paths.evidence_marker(),
    };
    let mut process = CrashProbeProcess::start(
        &std::env::current_exe()?,
        boundary,
        paths,
        postgres_url,
        organization_id,
        build_id,
    )?;
    for _ in 0..6000 {
        if marker.is_file() {
            return Ok(process.kill_and_wait()?);
        }
        if let Some(status) = process.try_wait()? {
            return Err(format!(
                "G0 {} crash probe exited before its durable marker with {status}",
                boundary.as_str()
            )
            .into());
        }
        tokio::time::sleep(StdDuration::from_millis(100)).await;
    }
    Err(format!(
        "G0 {} crash probe did not reach its durable boundary",
        boundary.as_str()
    )
    .into())
}

struct CrashProbeProcess {
    child: Option<Child>,
}

impl CrashProbeProcess {
    fn start(
        executable: &Path,
        boundary: CrashBoundary,
        paths: &ProbePaths,
        postgres_url: &str,
        organization_id: OrganizationId,
        build_id: BuildRunId,
    ) -> std::io::Result<Self> {
        let child = Command::new(executable)
            .arg(PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(POSTGRES_ENV, postgres_url)
            .env(PROBE_BOUNDARY_ENV, boundary.as_str())
            .env(PROBE_ROOT_ENV, &paths.root)
            .env(PROBE_ORGANIZATION_ENV, organization_id.to_string())
            .env(PROBE_BUILD_ENV, build_id.to_string())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("G0 crash probe process disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| std::io::Error::other("G0 crash probe process disappeared"))?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        child.kill()?;
        child.wait()
    }
}

impl Drop for CrashProbeProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) fn require_sigkill(status: ExitStatus, boundary: &str) -> Result<(), Box<dyn Error>> {
    require(!status.success(), format!("G0 {boundary} probe survived"))?;
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        require(
            status.signal() == Some(9),
            format!("G0 {boundary} probe exited with {status} instead of SIGKILL"),
        )?;
    }
    Ok(())
}

pub(super) struct CrashAfterPublication {
    pub(super) inner: Arc<dyn IBuildArtifactPublisher>,
    pub(super) marker: PathBuf,
}

#[async_trait]
impl IBuildArtifactPublisher for CrashAfterPublication {
    fn target_for(
        &self,
        build: &BuildRun,
    ) -> Result<OciPublicationTarget, BuildArtifactPublicationError> {
        self.inner.target_for(build)
    }

    async fn find(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<Option<PublishedOciArtifact>, BuildArtifactPublicationError> {
        self.inner.find(request).await
    }

    async fn publish(
        &self,
        request: &OciPublicationRequest,
    ) -> Result<PublishedOciArtifact, BuildArtifactPublicationError> {
        let published = self.inner.publish(request).await?;
        write_durable_json(&self.marker, &published)
            .map_err(|error| BuildArtifactPublicationError::Storage(error.to_string()))?;
        park_until_killed()
    }
}

pub(super) struct CrashAfterEvidenceSave {
    pub(super) inner: Arc<PostgresBuildRunRepository>,
    pub(super) marker: PathBuf,
}

#[async_trait]
impl IBuildRunRepository for CrashAfterEvidenceSave {
    async fn reserve_pending(
        &self,
        limit: usize,
        reserved_at: chrono::DateTime<Utc>,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        self.inner.reserve_pending(limit, reserved_at).await
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        self.inner.pending_operation_starts(limit).await
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<BuildRun, RepositoryError> {
        self.inner.find(organization_id, build_run_id).await
    }

    async fn find_by_source_revision(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        self.inner
            .find_by_source_revision(organization_id, source_revision_id)
            .await
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<BuildRun>, RepositoryError> {
        self.inner
            .list(organization_id, project_id, environment_id, limit)
            .await
    }

    async fn request_cancellation(
        &self,
        request: RequestBuildCancellationBundle,
    ) -> Result<IdempotentWrite<BuildRun>, RepositoryError> {
        self.inner.request_cancellation(request).await
    }

    async fn replay_cancellation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        self.inner.replay_cancellation(idempotency).await
    }

    async fn request_retry(
        &self,
        request: RequestBuildRetryBundle,
    ) -> Result<IdempotentWrite<BuildRun>, RepositoryError> {
        self.inner.request_retry(request).await
    }

    async fn replay_retry(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<BuildRun>, RepositoryError> {
        self.inner.replay_retry(idempotency).await
    }

    async fn save(
        &self,
        build_run: BuildRun,
        expected_version: u64,
    ) -> Result<BuildRun, RepositoryError> {
        let crash = build_run.evidence.is_some();
        let saved = self.inner.save(build_run, expected_version).await?;
        if crash {
            let evidence = saved
                .evidence
                .as_deref()
                .ok_or_else(|| RepositoryError::Storage("saved evidence disappeared".into()))?;
            write_durable_json(&self.marker, evidence)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?;
            park_until_killed()
        }
        Ok(saved)
    }
}

pub(super) struct StoredInputPreparer {
    pub(super) artifact: BuildArtifact,
}

#[async_trait]
impl IBuildInputPreparer for StoredInputPreparer {
    async fn prepare(
        &self,
        build: &BuildRun,
        revision: &ExternalSourceRevision,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        if build.organization_id != revision.organization_id
            || build.project_id != revision.project_id
            || build.environment_id != revision.environment_id
            || build.source_revision_id != revision.id
        {
            return Err(BuildInputPreparationError::Conflict);
        }
        Ok(PreparedBuildInput {
            source_content_digest: self.artifact.digest.clone(),
            artifact: self.artifact.clone(),
        })
    }

    async fn remove(&self, _build: &BuildRun) -> Result<(), BuildInputPreparationError> {
        Ok(())
    }
}

pub(super) struct NoopInputPreparer;

#[async_trait]
impl IBuildInputPreparer for NoopInputPreparer {
    async fn prepare(
        &self,
        _build: &BuildRun,
        _revision: &ExternalSourceRevision,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        Err(BuildInputPreparationError::Integrity(
            "recovery attempted to prepare input again".into(),
        ))
    }

    async fn remove(&self, _build: &BuildRun) -> Result<(), BuildInputPreparationError> {
        Ok(())
    }
}

pub(super) struct RejectingPublisher;

#[async_trait]
impl IBuildArtifactPublisher for RejectingPublisher {
    fn target_for(
        &self,
        _build: &BuildRun,
    ) -> Result<OciPublicationTarget, BuildArtifactPublicationError> {
        Err(BuildArtifactPublicationError::Integrity(
            "recovery attempted to derive a second publication".into(),
        ))
    }

    async fn find(
        &self,
        _request: &OciPublicationRequest,
    ) -> Result<Option<PublishedOciArtifact>, BuildArtifactPublicationError> {
        Err(BuildArtifactPublicationError::Integrity(
            "recovery attempted to look up publication after Flow completion".into(),
        ))
    }

    async fn publish(
        &self,
        _request: &OciPublicationRequest,
    ) -> Result<PublishedOciArtifact, BuildArtifactPublicationError> {
        Err(BuildArtifactPublicationError::Integrity(
            "recovery attempted to publish after Flow completion".into(),
        ))
    }
}

pub(super) struct RejectingEvidenceGenerator;

#[async_trait]
impl IBuildEvidenceGenerator for RejectingEvidenceGenerator {
    async fn generate(
        &self,
        _build: &BuildRun,
        _revision: &ExternalSourceRevision,
        _attested_at: chrono::DateTime<Utc>,
    ) -> Result<BuildEvidence, BuildEvidenceGenerationError> {
        Err(BuildEvidenceGenerationError::Integrity(
            "recovery attempted to sign a second evidence document".into(),
        ))
    }
}
