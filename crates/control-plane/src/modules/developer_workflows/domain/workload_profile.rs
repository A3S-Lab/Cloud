use super::source_layout::validate_repository_root;
use super::AcceptedBuildPlan;
use crate::modules::executions::domain::{
    ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate,
    MAX_EXECUTION_TIMEOUT_MS,
};
use crate::modules::shared_kernel::domain::{BuildPlanId, Sha256Digest, SourceRevisionId};
use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, OciArtifact, SecretBinding, SecretBindingTarget, ServicePort, ServiceProcess,
    ServiceResources, ServiceTemplate,
};
use a3s_acl::builder::{integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use chrono_tz::Tz;
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use uuid::Uuid;

pub const WORKLOAD_PROFILE_SCHEMA: &str = "a3s.cloud.workload-profile.v1";
pub const WORKLOAD_PROFILE_MAX_ACL_BYTES: usize = 128 * 1024;
const WORKLOAD_PROFILE_BLOCK: &str = "workload_profile";
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_SCHEDULE_EXPRESSION_BYTES: usize = 256;
const MAX_TIMEZONE_NAME_BYTES: usize = 255;
const MAX_SCHEDULE_CONCURRENCY: u16 = 64;
const MAX_SCHEDULE_MISFIRE_GRACE_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_SCHEDULE_ATTEMPTS: u16 = 8;
const MAX_SCHEDULE_BACKOFF_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_SCHEDULE_HISTORY_COUNT: u16 = 10_000;
const MAX_SCHEDULE_HISTORY_DAYS: u16 = 3_650;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadProfileKind {
    Web,
    Worker,
    ScheduledTask,
}

impl WorkloadProfileKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Worker => "worker",
            Self::ScheduledTask => "scheduled_task",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "web" => Ok(Self::Web),
            "worker" => Ok(Self::Worker),
            "scheduled_task" => Ok(Self::ScheduledTask),
            _ => Err("workload profile kind is unsupported".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledTaskCatchUpPolicy {
    Skip,
    Latest,
}

impl ScheduledTaskCatchUpPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Latest => "latest",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "skip" => Ok(Self::Skip),
            "latest" => Ok(Self::Latest),
            _ => Err("scheduled Task catch-up policy is unsupported".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledTaskRetryPolicy {
    pub maximum_attempts: u16,
    pub initial_backoff_ms: u64,
    pub maximum_backoff_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledTaskHistoryPolicy {
    pub successful_limit: u16,
    pub failed_limit: u16,
    pub maximum_age_days: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledTaskSchedule {
    pub expression: String,
    pub timezone: String,
    pub catch_up: ScheduledTaskCatchUpPolicy,
    pub maximum_concurrency: u16,
    pub misfire_grace_ms: u64,
    pub retry: ScheduledTaskRetryPolicy,
    pub history: ScheduledTaskHistoryPolicy,
}

impl ScheduledTaskSchedule {
    pub fn validate(&self) -> Result<(), String> {
        let normalized = self
            .expression
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if self.expression.is_empty()
            || self.expression.len() > MAX_SCHEDULE_EXPRESSION_BYTES
            || self.expression != normalized
            || self.expression.split_ascii_whitespace().count() != 7
            || Schedule::from_str(&self.expression).is_err()
        {
            return Err(
                "scheduled Task expression must be one canonical seven-field cron expression"
                    .into(),
            );
        }
        if self.timezone.is_empty()
            || self.timezone.len() > MAX_TIMEZONE_NAME_BYTES
            || self.timezone.contains(['\0', '\r', '\n', '\t', ' '])
        {
            return Err("scheduled Task timezone name is invalid".into());
        }
        let timezone = Tz::from_str(&self.timezone)
            .map_err(|_| "scheduled Task timezone must be an IANA timezone name".to_owned())?;
        if timezone.name() != self.timezone {
            return Err("scheduled Task timezone name is not canonical".into());
        }
        if self.maximum_concurrency == 0
            || self.maximum_concurrency > MAX_SCHEDULE_CONCURRENCY
            || self.misfire_grace_ms == 0
            || self.misfire_grace_ms > MAX_SCHEDULE_MISFIRE_GRACE_MS
            || self.retry.maximum_attempts == 0
            || self.retry.maximum_attempts > MAX_SCHEDULE_ATTEMPTS
            || self.retry.initial_backoff_ms == 0
            || self.retry.initial_backoff_ms > self.retry.maximum_backoff_ms
            || self.retry.maximum_backoff_ms > MAX_SCHEDULE_BACKOFF_MS
            || self.history.successful_limit > MAX_SCHEDULE_HISTORY_COUNT
            || self.history.failed_limit > MAX_SCHEDULE_HISTORY_COUNT
            || self.history.successful_limit == 0 && self.history.failed_limit == 0
            || self.history.maximum_age_days == 0
            || self.history.maximum_age_days > MAX_SCHEDULE_HISTORY_DAYS
        {
            return Err("scheduled Task policy is outside the closed P0 bounds".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadProfileResources {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub ephemeral_storage_bytes: Option<u64>,
    pub execution_timeout_ms: Option<u64>,
}

impl WorkloadProfileResources {
    pub(crate) fn service_resources(&self) -> ServiceResources {
        ServiceResources {
            cpu_millis: self.cpu_millis,
            memory_bytes: self.memory_bytes,
            pids: self.pids,
            ephemeral_storage_bytes: self.ephemeral_storage_bytes,
        }
    }

    pub(crate) fn execution_resources(&self) -> Result<ExecutionResources, String> {
        Ok(ExecutionResources {
            cpu_millis: self.cpu_millis,
            memory_bytes: self.memory_bytes,
            pids: self.pids,
            ephemeral_storage_bytes: self.ephemeral_storage_bytes,
            timeout_ms: self
                .execution_timeout_ms
                .ok_or_else(|| "scheduled Task profile requires an execution timeout".to_owned())?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadProfileSpec {
    pub name: String,
    pub kind: WorkloadProfileKind,
    pub process: ServiceProcess,
    pub secrets: Vec<SecretBinding>,
    pub resources: WorkloadProfileResources,
    pub ports: Vec<ServicePort>,
    pub health: Option<HttpHealthCheck>,
    pub public_port: Option<String>,
    pub schedule: Option<ScheduledTaskSchedule>,
}

impl WorkloadProfileSpec {
    pub fn validate(&self) -> Result<(), String> {
        validate_profile_name(&self.name)?;
        match self.kind {
            WorkloadProfileKind::Web => {
                if self
                    .public_port
                    .as_ref()
                    .is_none_or(|port| !self.ports.iter().any(|candidate| &candidate.name == port))
                    || self.health.is_none()
                    || self.schedule.is_some()
                    || self.resources.execution_timeout_ms.is_some()
                {
                    return Err(
                        "web profile requires a public port and health check but no Task policy"
                            .into(),
                    );
                }
                validate_service_profile(self)?;
            }
            WorkloadProfileKind::Worker => {
                if self.public_port.is_some()
                    || self.schedule.is_some()
                    || self.resources.execution_timeout_ms.is_some()
                {
                    return Err("worker profile cannot own a Route or Task policy".into());
                }
                validate_service_profile(self)?;
            }
            WorkloadProfileKind::ScheduledTask => {
                if self.public_port.is_some()
                    || !self.ports.is_empty()
                    || self.health.is_some()
                    || !self.secrets.is_empty()
                {
                    return Err(
                        "scheduled Task profile cannot own a Route, Service port, health check, or public Execution Secret"
                            .into(),
                    );
                }
                self.schedule
                    .as_ref()
                    .ok_or_else(|| "scheduled Task profile requires a schedule".to_owned())?
                    .validate()?;
                validate_scheduled_task_profile(self)?;
            }
        }
        Ok(())
    }

    pub(crate) fn project_service_template(&self, artifact: OciArtifact) -> ServiceTemplate {
        ServiceTemplate {
            artifact,
            process: self.process.clone(),
            secrets: self.secrets.clone(),
            resources: self.resources.service_resources(),
            ports: self.ports.clone(),
            health: self.health.clone(),
        }
    }

    pub(crate) fn project_execution_template(
        &self,
        artifact: OciArtifact,
    ) -> Result<ExecutionTemplate, String> {
        Ok(ExecutionTemplate {
            artifact: ExecutionArtifact {
                uri: artifact.uri,
                digest: artifact.digest,
                media_type: artifact.media_type,
            },
            process: ExecutionProcess {
                command: self.process.command.clone(),
                args: self.process.args.clone(),
                working_directory: self.process.working_directory.clone(),
                environment: self.process.environment.clone(),
            },
            input: serde_json::Value::Null,
            resources: self.resources.execution_resources()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadProfileContractSpec {
    pub build_plan_id: BuildPlanId,
    pub build_plan_digest: Sha256Digest,
    pub source_revision_id: SourceRevisionId,
    pub project_root: String,
    pub profile: WorkloadProfileSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadProfileContract {
    spec: WorkloadProfileContractSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkloadProfileContract {
    pub fn bind(
        build_plan: &AcceptedBuildPlan,
        profile: WorkloadProfileSpec,
    ) -> Result<Self, String> {
        build_plan.validate()?;
        let proposal = &build_plan.contract.spec().proposal;
        Self::from_spec(WorkloadProfileContractSpec {
            build_plan_id: build_plan.id,
            build_plan_digest: build_plan.contract.digest().clone(),
            source_revision_id: build_plan.source_revision_id,
            project_root: proposal.spec().project_root.clone(),
            profile,
        })
    }

    fn from_spec(mut spec: WorkloadProfileContractSpec) -> Result<Self, String> {
        validate_contract_binding(&spec)?;
        spec.profile
            .secrets
            .sort_by(|left, right| left.name.cmp(&right.name));
        spec.profile
            .ports
            .sort_by(|left, right| left.name.cmp(&right.name));
        spec.profile.validate()?;
        let document = profile_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > WORKLOAD_PROFILE_MAX_ACL_BYTES {
            return Err("workload profile ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated workload profile ACL is invalid: {error}"))?;
        let digest =
            Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
                format!("workload profile ACL is not canonicalizable: {error}")
            })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > WORKLOAD_PROFILE_MAX_ACL_BYTES {
            return Err("workload profile ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("workload profile ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("workload profile ACL is invalid: {error}"))?;
        let value = Self::from_spec(parse_profile(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("workload profile ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored workload profile ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("workload profile drifted from its canonical ACL".into());
        }
        Ok(())
    }

    pub fn validate_for(&self, build_plan: &AcceptedBuildPlan) -> Result<(), String> {
        self.validate()?;
        build_plan.validate()?;
        let proposal = &build_plan.contract.spec().proposal;
        if self.spec.build_plan_id != build_plan.id
            || self.spec.build_plan_digest != *build_plan.contract.digest()
            || self.spec.source_revision_id != build_plan.source_revision_id
            || self.spec.project_root != proposal.spec().project_root
        {
            return Err("workload profile changed its accepted BuildPlan binding".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &WorkloadProfileContractSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn validate_contract_binding(spec: &WorkloadProfileContractSpec) -> Result<(), String> {
    if spec.build_plan_id.as_uuid().is_nil()
        || spec.source_revision_id.as_uuid().is_nil()
        || Sha256Digest::parse(spec.build_plan_digest.as_str())? != spec.build_plan_digest
    {
        return Err("workload profile BuildPlan binding is invalid".into());
    }
    validate_repository_root(&spec.project_root)
}

fn validate_profile_name(name: &str) -> Result<(), String> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 63
        || !bytes[0].is_ascii_lowercase()
        || !bytes[bytes.len() - 1].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    {
        return Err("workload profile name must be a lowercase DNS-style identifier".into());
    }
    Ok(())
}

fn validation_artifact() -> OciArtifact {
    let digest = format!("sha256:{}", "0".repeat(64));
    OciArtifact {
        uri: format!("oci://validation.invalid/a3s/workload-profile@{digest}"),
        digest,
        media_type: "application/vnd.oci.image.manifest.v1+json".into(),
    }
}

fn validate_service_profile(spec: &WorkloadProfileSpec) -> Result<(), String> {
    spec.project_service_template(validation_artifact())
        .validate()
}

fn validate_scheduled_task_profile(spec: &WorkloadProfileSpec) -> Result<(), String> {
    spec.project_execution_template(validation_artifact())?
        .validate()?;
    if spec
        .resources
        .execution_timeout_ms
        .is_some_and(|timeout| timeout > MAX_EXECUTION_TIMEOUT_MS)
    {
        return Err("scheduled Task execution timeout exceeds the owner contract".into());
    }
    Ok(())
}

fn profile_document(spec: &WorkloadProfileContractSpec) -> Result<Document, String> {
    let profile = &spec.profile;
    let mut process = BlockBuilder::new("process")
        .attr(
            "args",
            list(
                profile
                    .process
                    .args
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        )
        .attr(
            "command",
            list(
                profile
                    .process
                    .command
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        );
    if let Some(directory) = &profile.process.working_directory {
        process = process.attr("working_directory", string(directory));
    }
    for (name, value) in &profile.process.environment {
        process = process.nested_block(
            BlockBuilder::new("environment")
                .label(name)
                .attr("value", string(value))
                .build(),
        );
    }

    let mut resources = BlockBuilder::new("resources")
        .attr(
            "cpu_millis",
            acl_integer("cpu_millis", profile.resources.cpu_millis)?,
        )
        .attr(
            "memory_bytes",
            acl_integer("memory_bytes", profile.resources.memory_bytes)?,
        )
        .attr(
            "pids",
            acl_integer("pids", u64::from(profile.resources.pids))?,
        );
    if let Some(bytes) = profile.resources.ephemeral_storage_bytes {
        resources = resources.attr(
            "ephemeral_storage_bytes",
            acl_integer("ephemeral_storage_bytes", bytes)?,
        );
    }
    if let Some(timeout) = profile.resources.execution_timeout_ms {
        resources = resources.attr(
            "execution_timeout_ms",
            acl_integer("execution_timeout_ms", timeout)?,
        );
    }

    let mut root = BlockBuilder::new(WORKLOAD_PROFILE_BLOCK)
        .label(&profile.name)
        .attr("build_plan_digest", string(spec.build_plan_digest.as_str()))
        .attr("build_plan_id", string(&spec.build_plan_id.to_string()))
        .attr("kind", string(profile.kind.as_str()))
        .attr("project_root", string(&spec.project_root))
        .attr("schema", string(WORKLOAD_PROFILE_SCHEMA))
        .attr(
            "source_revision_id",
            string(&spec.source_revision_id.to_string()),
        )
        .nested_block(process.build())
        .nested_block(resources.build());

    for secret in &profile.secrets {
        root = root.nested_block(secret_block(secret)?);
    }
    for port in &profile.ports {
        root = root.nested_block(
            BlockBuilder::new("port")
                .label(&port.name)
                .attr(
                    "container_port",
                    acl_integer("container_port", u64::from(port.container_port))?,
                )
                .build(),
        );
    }
    if let Some(health) = &profile.health {
        root = root.nested_block(
            BlockBuilder::new("health")
                .attr(
                    "healthy_threshold",
                    acl_integer("healthy_threshold", u64::from(health.healthy_threshold))?,
                )
                .attr(
                    "interval_ms",
                    acl_integer("interval_ms", health.interval_ms)?,
                )
                .attr("path", string(&health.path))
                .attr("port", string(&health.port_name))
                .attr(
                    "stabilization_window_ms",
                    acl_integer("stabilization_window_ms", health.stabilization_window_ms)?,
                )
                .attr("timeout_ms", acl_integer("timeout_ms", health.timeout_ms)?)
                .attr(
                    "unhealthy_threshold",
                    acl_integer("unhealthy_threshold", u64::from(health.unhealthy_threshold))?,
                )
                .build(),
        );
    }
    if let Some(port) = &profile.public_port {
        root = root.nested_block(
            BlockBuilder::new("route")
                .attr("port", string(port))
                .build(),
        );
    }
    if let Some(schedule) = &profile.schedule {
        root = root.nested_block(schedule_block(schedule)?);
    }
    Ok(Document {
        blocks: vec![root.build()],
    })
}

fn secret_block(secret: &SecretBinding) -> Result<Block, String> {
    let mut block = BlockBuilder::new("secret")
        .label(&secret.name)
        .attr("secret_id", string(&secret.secret_id.to_string()))
        .attr("version", acl_integer("version", secret.version)?);
    block = match &secret.target {
        SecretBindingTarget::Environment { variable } => block
            .attr("target", string("environment"))
            .attr("variable", string(variable)),
        SecretBindingTarget::File { path, mode } => block
            .attr("mode", acl_integer("mode", u64::from(*mode))?)
            .attr("path", string(path))
            .attr("target", string("file")),
        SecretBindingTarget::RegistryCredential => {
            block.attr("target", string("registry_credential"))
        }
    };
    Ok(block.build())
}

fn schedule_block(schedule: &ScheduledTaskSchedule) -> Result<Block, String> {
    Ok(BlockBuilder::new("schedule")
        .attr("catch_up", string(schedule.catch_up.as_str()))
        .attr("expression", string(&schedule.expression))
        .attr(
            "maximum_concurrency",
            acl_integer(
                "maximum_concurrency",
                u64::from(schedule.maximum_concurrency),
            )?,
        )
        .attr(
            "misfire_grace_ms",
            acl_integer("misfire_grace_ms", schedule.misfire_grace_ms)?,
        )
        .attr("timezone", string(&schedule.timezone))
        .nested_block(
            BlockBuilder::new("retry")
                .attr(
                    "initial_backoff_ms",
                    acl_integer("initial_backoff_ms", schedule.retry.initial_backoff_ms)?,
                )
                .attr(
                    "maximum_attempts",
                    acl_integer(
                        "maximum_attempts",
                        u64::from(schedule.retry.maximum_attempts),
                    )?,
                )
                .attr(
                    "maximum_backoff_ms",
                    acl_integer("maximum_backoff_ms", schedule.retry.maximum_backoff_ms)?,
                )
                .build(),
        )
        .nested_block(
            BlockBuilder::new("history")
                .attr(
                    "failed_limit",
                    acl_integer_allow_zero(
                        "failed_limit",
                        u64::from(schedule.history.failed_limit),
                    )?,
                )
                .attr(
                    "maximum_age_days",
                    acl_integer(
                        "maximum_age_days",
                        u64::from(schedule.history.maximum_age_days),
                    )?,
                )
                .attr(
                    "successful_limit",
                    acl_integer_allow_zero(
                        "successful_limit",
                        u64::from(schedule.history.successful_limit),
                    )?,
                )
                .build(),
        )
        .build())
}

fn parse_profile(document: &Document) -> Result<WorkloadProfileContractSpec, String> {
    if document.blocks.len() != 1 {
        return Err("workload profile must contain one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_root(root)?;
    if required_string(root, "schema")? != WORKLOAD_PROFILE_SCHEMA {
        return Err("workload profile schema is unsupported".into());
    }
    let process = parse_process(exact_child(root, "process")?)?;
    let resources = parse_resources(exact_child(root, "resources")?)?;
    let secrets = root
        .blocks
        .iter()
        .filter(|block| block.name == "secret")
        .map(parse_secret)
        .collect::<Result<Vec<_>, _>>()?;
    let ports = root
        .blocks
        .iter()
        .filter(|block| block.name == "port")
        .map(parse_port)
        .collect::<Result<Vec<_>, _>>()?;
    let health = optional_child(root, "health")?
        .map(parse_health)
        .transpose()?;
    let public_port = optional_child(root, "route")?
        .map(|block| {
            exact_block(block, "route", &["port"], 0, &[])?;
            required_string(block, "port")
        })
        .transpose()?;
    let schedule = optional_child(root, "schedule")?
        .map(parse_schedule)
        .transpose()?;
    Ok(WorkloadProfileContractSpec {
        build_plan_id: BuildPlanId::from_uuid(required_uuid(root, "build_plan_id")?),
        build_plan_digest: Sha256Digest::parse(required_string(root, "build_plan_digest")?)?,
        source_revision_id: SourceRevisionId::from_uuid(required_uuid(root, "source_revision_id")?),
        project_root: required_string(root, "project_root")?,
        profile: WorkloadProfileSpec {
            name: root.labels[0].clone(),
            kind: WorkloadProfileKind::parse(&required_string(root, "kind")?)?,
            process,
            secrets,
            resources,
            ports,
            health,
            public_port,
            schedule,
        },
    })
}

fn exact_root(root: &Block) -> Result<(), String> {
    const ATTRIBUTES: &[&str] = &[
        "build_plan_digest",
        "build_plan_id",
        "kind",
        "project_root",
        "schema",
        "source_revision_id",
    ];
    const CHILDREN: &[&str] = &[
        "process",
        "resources",
        "secret",
        "port",
        "health",
        "route",
        "schedule",
    ];
    if root.name != WORKLOAD_PROFILE_BLOCK
        || root.labels.len() != 1
        || root.attributes.len() != ATTRIBUTES.len()
        || root
            .attributes
            .keys()
            .any(|key| !ATTRIBUTES.contains(&key.as_str()))
        || root
            .blocks
            .iter()
            .any(|block| !CHILDREN.contains(&block.name.as_str()))
    {
        return Err("workload profile root block shape is invalid".into());
    }
    exact_child(root, "process")?;
    exact_child(root, "resources")?;
    optional_child(root, "health")?;
    optional_child(root, "route")?;
    optional_child(root, "schedule")?;
    Ok(())
}

fn parse_process(block: &Block) -> Result<ServiceProcess, String> {
    let keys = block
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if block.name != "process"
        || !block.labels.is_empty()
        || !BTreeSet::from(["args", "command"]).is_subset(&keys)
        || !keys.is_subset(&BTreeSet::from(["args", "command", "working_directory"]))
        || block.blocks.iter().any(|child| child.name != "environment")
    {
        return Err("workload profile process block shape is invalid".into());
    }
    let mut environment = BTreeMap::new();
    for variable in &block.blocks {
        exact_block(variable, "environment", &["value"], 1, &[])?;
        if environment
            .insert(
                variable.labels[0].clone(),
                required_string(variable, "value")?,
            )
            .is_some()
        {
            return Err("workload profile contains duplicate environment variables".into());
        }
    }
    Ok(ServiceProcess {
        command: required_strings(block, "command")?,
        args: required_strings(block, "args")?,
        working_directory: optional_string(block, "working_directory")?,
        environment,
    })
}

fn parse_resources(block: &Block) -> Result<WorkloadProfileResources, String> {
    let keys = block
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let required = BTreeSet::from(["cpu_millis", "memory_bytes", "pids"]);
    let allowed = BTreeSet::from([
        "cpu_millis",
        "memory_bytes",
        "pids",
        "ephemeral_storage_bytes",
        "execution_timeout_ms",
    ]);
    if block.name != "resources"
        || !block.labels.is_empty()
        || !block.blocks.is_empty()
        || !required.is_subset(&keys)
        || !keys.is_subset(&allowed)
    {
        return Err("workload profile resources block shape is invalid".into());
    }
    Ok(WorkloadProfileResources {
        cpu_millis: required_u64(block, "cpu_millis")?,
        memory_bytes: required_u64(block, "memory_bytes")?,
        pids: required_u32(block, "pids")?,
        ephemeral_storage_bytes: optional_u64(block, "ephemeral_storage_bytes")?,
        execution_timeout_ms: optional_u64(block, "execution_timeout_ms")?,
    })
}

fn parse_secret(block: &Block) -> Result<SecretBinding, String> {
    if block.name != "secret" || block.labels.len() != 1 || !block.blocks.is_empty() {
        return Err("workload profile Secret block shape is invalid".into());
    }
    let target = required_string(block, "target")?;
    let expected = match target.as_str() {
        "environment" => &["secret_id", "target", "variable", "version"][..],
        "file" => &["mode", "path", "secret_id", "target", "version"][..],
        "registry_credential" => &["secret_id", "target", "version"][..],
        _ => return Err("workload profile Secret target is unsupported".into()),
    };
    if block.attributes.len() != expected.len()
        || block
            .attributes
            .keys()
            .any(|key| !expected.contains(&key.as_str()))
    {
        return Err("workload profile Secret block attributes are invalid".into());
    }
    let target = match target.as_str() {
        "environment" => SecretBindingTarget::Environment {
            variable: required_string(block, "variable")?,
        },
        "file" => SecretBindingTarget::File {
            path: required_string(block, "path")?,
            mode: required_u32(block, "mode")?,
        },
        "registry_credential" => SecretBindingTarget::RegistryCredential,
        _ => unreachable!("closed target was matched above"),
    };
    Ok(SecretBinding {
        name: block.labels[0].clone(),
        secret_id: crate::modules::shared_kernel::domain::SecretId::from_uuid(required_uuid(
            block,
            "secret_id",
        )?),
        version: required_u64(block, "version")?,
        target,
    })
}

fn parse_port(block: &Block) -> Result<ServicePort, String> {
    exact_block(block, "port", &["container_port"], 1, &[])?;
    Ok(ServicePort {
        name: block.labels[0].clone(),
        container_port: u16::try_from(required_u64(block, "container_port")?)
            .map_err(|_| "workload profile container port exceeds u16".to_owned())?,
    })
}

fn parse_health(block: &Block) -> Result<HttpHealthCheck, String> {
    exact_block(
        block,
        "health",
        &[
            "healthy_threshold",
            "interval_ms",
            "path",
            "port",
            "stabilization_window_ms",
            "timeout_ms",
            "unhealthy_threshold",
        ],
        0,
        &[],
    )?;
    Ok(HttpHealthCheck {
        port_name: required_string(block, "port")?,
        path: required_string(block, "path")?,
        interval_ms: required_u64(block, "interval_ms")?,
        timeout_ms: required_u64(block, "timeout_ms")?,
        healthy_threshold: u16::try_from(required_u64(block, "healthy_threshold")?)
            .map_err(|_| "workload profile healthy threshold exceeds u16".to_owned())?,
        unhealthy_threshold: u16::try_from(required_u64(block, "unhealthy_threshold")?)
            .map_err(|_| "workload profile unhealthy threshold exceeds u16".to_owned())?,
        stabilization_window_ms: required_u64(block, "stabilization_window_ms")?,
    })
}

fn parse_schedule(block: &Block) -> Result<ScheduledTaskSchedule, String> {
    exact_block(
        block,
        "schedule",
        &[
            "catch_up",
            "expression",
            "maximum_concurrency",
            "misfire_grace_ms",
            "timezone",
        ],
        0,
        &["retry", "history"],
    )?;
    let retry = exact_child(block, "retry")?;
    exact_block(
        retry,
        "retry",
        &[
            "initial_backoff_ms",
            "maximum_attempts",
            "maximum_backoff_ms",
        ],
        0,
        &[],
    )?;
    let history = exact_child(block, "history")?;
    exact_block(
        history,
        "history",
        &["failed_limit", "maximum_age_days", "successful_limit"],
        0,
        &[],
    )?;
    Ok(ScheduledTaskSchedule {
        expression: required_string(block, "expression")?,
        timezone: required_string(block, "timezone")?,
        catch_up: ScheduledTaskCatchUpPolicy::parse(&required_string(block, "catch_up")?)?,
        maximum_concurrency: required_u16(block, "maximum_concurrency")?,
        misfire_grace_ms: required_u64(block, "misfire_grace_ms")?,
        retry: ScheduledTaskRetryPolicy {
            maximum_attempts: required_u16(retry, "maximum_attempts")?,
            initial_backoff_ms: required_u64(retry, "initial_backoff_ms")?,
            maximum_backoff_ms: required_u64(retry, "maximum_backoff_ms")?,
        },
        history: ScheduledTaskHistoryPolicy {
            successful_limit: required_u16_allow_zero(history, "successful_limit")?,
            failed_limit: required_u16_allow_zero(history, "failed_limit")?,
            maximum_age_days: required_u16(history, "maximum_age_days")?,
        },
    })
}

fn exact_child<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    optional_child(root, name)?.ok_or_else(|| format!("workload profile {name} block is required"))
}

fn optional_child<'a>(root: &'a Block, name: &str) -> Result<Option<&'a Block>, String> {
    let mut matching = root.blocks.iter().filter(|block| block.name == name);
    let value = matching.next();
    if matching.next().is_some() {
        return Err(format!("workload profile {name} block must be unique"));
    }
    Ok(value)
}

fn exact_block(
    block: &Block,
    name: &str,
    attributes: &[&str],
    labels: usize,
    children: &[&str],
) -> Result<(), String> {
    if block.name != name
        || block.labels.len() != labels
        || block.attributes.len() != attributes.len()
        || block
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || block.blocks.len() != children.len()
        || block
            .blocks
            .iter()
            .any(|child| !children.contains(&child.name.as_str()))
    {
        return Err(format!("workload profile {name} block shape is invalid"));
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("workload profile field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("workload profile field {name:?} must be a string"))
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("workload profile field {name:?} must be a string"))
        })
        .transpose()
}

fn required_strings(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "workload profile field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("workload profile field {name:?} must be a string list"))
        })
        .collect()
}

fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    Uuid::parse_str(&required_string(block, name)?)
        .map_err(|_| format!("workload profile field {name:?} must be a UUID"))
}

fn number(block: &Block, name: &str, allow_zero: bool) -> Result<u64, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("workload profile field {name:?} must be an integer"))?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value < 0.0
        || !allow_zero && value == 0.0
        || value > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "workload profile field {name:?} must be an exactly representable bounded integer"
        ));
    }
    Ok(value as u64)
}

fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    number(block, name, false)
}

fn required_u32(block: &Block, name: &str) -> Result<u32, String> {
    u32::try_from(required_u64(block, name)?)
        .map_err(|_| format!("workload profile field {name:?} exceeds u32"))
}

fn required_u16(block: &Block, name: &str) -> Result<u16, String> {
    u16::try_from(required_u64(block, name)?)
        .map_err(|_| format!("workload profile field {name:?} exceeds u16"))
}

fn required_u16_allow_zero(block: &Block, name: &str) -> Result<u16, String> {
    u16::try_from(number(block, name, true)?)
        .map_err(|_| format!("workload profile field {name:?} exceeds u16"))
}

fn optional_u64(block: &Block, name: &str) -> Result<Option<u64>, String> {
    block
        .attributes
        .get(name)
        .map(|_| required_u64(block, name))
        .transpose()
}

fn acl_integer(name: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "workload profile field {name:?} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}

fn acl_integer_allow_zero(name: &str, value: u64) -> Result<Value, String> {
    if value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "workload profile field {name:?} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}
