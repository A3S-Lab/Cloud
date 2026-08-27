use super::{AgentProviderCapabilityV1, AgentProviderProfile, HarnessInvocationProfileV1};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const AGENT_PROVIDER_MAX_PROMPT_BYTES: usize = 1024 * 1024;

pub const AGENT_PROVIDER_COMMAND_HTTP_PATH_V1: &str = "/v1/agent-provider/commands";
pub const AGENT_PROVIDER_MAX_COMMAND_RECEIPT_BYTES: usize = 64 * 1024;
/// Fixed v1 expiry policy for approval-required Tool requests.
pub const AGENT_PROVIDER_TOOL_APPROVAL_TTL_MS_V1: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderRunStateV1 {
    Created,
    Planning,
    Executing,
    AwaitingApproval,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderApprovalOutcomeV1 {
    Approved,
    Denied,
    Expired,
}

impl AgentProviderApprovalOutcomeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            _ => Err(format!(
                "unsupported Agent provider approval outcome {value:?}"
            )),
        }
    }
}

/// One immutable Cloud decision for one approval-required provider Tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderApprovalDecisionV1 {
    pub schema: String,
    pub decision_id: String,
    pub checkpoint_id: String,
    pub run_identity_digest: String,
    pub call_id: String,
    pub tool: super::HarnessToolBindingV1,
    pub request_digest: String,
    pub outcome: AgentProviderApprovalOutcomeV1,
    pub decided_at_ms: u64,
}

impl AgentProviderApprovalDecisionV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-approval-decision.v1";

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        decision_id: String,
        checkpoint_id: String,
        identity: &AgentProviderRunIdentityV1,
        call_id: String,
        tool: super::HarnessToolBindingV1,
        request_digest: String,
        outcome: AgentProviderApprovalOutcomeV1,
        decided_at_ms: u64,
    ) -> Result<Self, String> {
        let decision = Self {
            schema: Self::SCHEMA.into(),
            decision_id,
            checkpoint_id,
            run_identity_digest: identity.digest()?,
            call_id,
            tool,
            request_digest,
            outcome,
            decided_at_ms,
        };
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider approval decision schema {:?}",
                self.schema
            ));
        }
        validate_line(
            "Agent provider approval decision ID",
            &self.decision_id,
            256,
        )?;
        validate_line(
            "Agent provider approval checkpoint ID",
            &self.checkpoint_id,
            256,
        )?;
        validate_digest(
            "Agent provider approval run identity digest",
            &self.run_identity_digest,
        )?;
        validate_line("Agent provider approval Tool call ID", &self.call_id, 256)?;
        self.tool.validate()?;
        if !self.tool.approval_required {
            return Err("Agent provider approval decision targets a Tool without approval".into());
        }
        validate_digest(
            "Agent provider approval Tool request digest",
            &self.request_digest,
        )?;
        if self.decided_at_ms == 0 {
            return Err("Agent provider approval decision time must be positive".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(|error| {
            format!("could not encode Agent provider approval decision: {error}")
        })?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

impl AgentProviderRunStateV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Planning => "planning",
            Self::Executing => "executing",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Verifying => "verifying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "created" => Ok(Self::Created),
            "planning" => Ok(Self::Planning),
            "executing" => Ok(Self::Executing),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "verifying" => Ok(Self::Verifying),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unsupported Agent provider run state {value:?}")),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderRunIdentityV1 {
    pub schema: String,
    pub provider_profile_digest: String,
    pub provider_capability_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_profile_digest: Option<String>,
    pub agent_release_identity: String,
    pub session_id: String,
    pub run_id: String,
}

impl AgentProviderRunIdentityV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-run-identity.v1";

    pub fn new(
        provider_profile_digest: String,
        provider_capability_digest: String,
        agent_release_identity: String,
        session_id: String,
        run_id: String,
    ) -> Result<Self, String> {
        let identity = Self {
            schema: Self::SCHEMA.into(),
            provider_profile_digest,
            provider_capability_digest,
            invocation_profile_digest: None,
            agent_release_identity,
            session_id,
            run_id,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider run identity schema {:?}",
                self.schema
            ));
        }
        validate_digest("provider profile digest", &self.provider_profile_digest)?;
        validate_digest(
            "provider capability digest",
            &self.provider_capability_digest,
        )?;
        if let Some(digest) = &self.invocation_profile_digest {
            validate_digest("Harness invocation profile digest", digest)?;
        }
        validate_digest("Agent release identity", &self.agent_release_identity)?;
        validate_line("Agent provider session ID", &self.session_id, 256)?;
        validate_line("Agent provider run ID", &self.run_id, 256)
    }

    pub fn validate_for(&self, profile: &AgentProviderProfile) -> Result<(), String> {
        self.validate()?;
        profile.validate()?;
        if self.provider_profile_digest != profile.digest()
            || self.provider_capability_digest != profile.capability_digest()
        {
            return Err("Agent provider run identity does not match its immutable profile".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode Agent provider run identity: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderRunStartV1 {
    pub schema: String,
    pub request_id: String,
    pub identity: AgentProviderRunIdentityV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_profile: Option<HarnessInvocationProfileV1>,
    pub prompt: String,
}

impl AgentProviderRunStartV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-run-start.v1";

    /// Retains decoding/construction compatibility for provider runs created
    /// before the A1.4 invocation-profile binding existed.
    pub fn new(
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        prompt: String,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            request_id,
            identity,
            invocation_profile: None,
            prompt,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn new_with_invocation_profile(
        request_id: String,
        mut identity: AgentProviderRunIdentityV1,
        invocation_profile: HarnessInvocationProfileV1,
        prompt: String,
    ) -> Result<Self, String> {
        let digest = invocation_profile.digest()?;
        match identity.invocation_profile_digest.as_deref() {
            Some(existing) if existing != digest.as_str() => {
                return Err(
                    "Agent provider run identity changed its Harness invocation profile".into(),
                )
            }
            Some(_) => {}
            None => identity.invocation_profile_digest = Some(digest),
        }
        let request = Self {
            schema: Self::SCHEMA.into(),
            request_id,
            identity,
            invocation_profile: Some(invocation_profile),
            prompt,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider start schema {:?}",
                self.schema
            ));
        }
        validate_line("Agent provider request ID", &self.request_id, 256)?;
        self.identity.validate()?;
        match (
            self.identity.invocation_profile_digest.as_deref(),
            self.invocation_profile.as_ref(),
        ) {
            (None, None) => {}
            (Some(expected), Some(profile))
                if profile.digest()? == expected
                    && profile.provider.profile_digest == self.identity.provider_profile_digest
                    && profile.provider.capability_digest
                        == self.identity.provider_capability_digest
                    && profile.agent.artifact_digest == self.identity.agent_release_identity => {}
            _ => {
                return Err(
                    "Agent provider start does not match its Harness invocation profile".into(),
                )
            }
        }
        if self.prompt.trim().is_empty()
            || self.prompt.len() > AGENT_PROVIDER_MAX_PROMPT_BYTES
            || self.prompt.contains('\0')
        {
            return Err("Agent provider prompt bounds are invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderRunCancelV1 {
    pub schema: String,
    pub request_id: String,
    pub identity: AgentProviderRunIdentityV1,
    pub reason: String,
}

impl AgentProviderRunCancelV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-run-cancel.v1";

    pub fn new(
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        reason: String,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            request_id,
            identity,
            reason,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider cancellation schema {:?}",
                self.schema
            ));
        }
        validate_line("Agent provider request ID", &self.request_id, 256)?;
        self.identity.validate()?;
        validate_line("Agent provider cancellation reason", &self.reason, 1024)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderRunRecoverV1 {
    pub schema: String,
    pub request_id: String,
    pub identity: AgentProviderRunIdentityV1,
    pub checkpoint_run_id: String,
}

impl AgentProviderRunRecoverV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-run-recover.v1";

    pub fn new(
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        checkpoint_run_id: String,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            request_id,
            identity,
            checkpoint_run_id,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider recovery schema {:?}",
                self.schema
            ));
        }
        validate_line("Agent provider request ID", &self.request_id, 256)?;
        self.identity.validate()?;
        validate_line(
            "Agent provider recovery checkpoint run ID",
            &self.checkpoint_run_id,
            256,
        )?;
        if self.checkpoint_run_id == self.identity.run_id {
            return Err("Agent provider recovery checkpoint cannot be the successor run".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderRunResumeV1 {
    pub schema: String,
    pub request_id: String,
    pub identity: AgentProviderRunIdentityV1,
    pub decision: AgentProviderApprovalDecisionV1,
}

impl AgentProviderRunResumeV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-run-resume.v1";

    pub fn new(
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        decision: AgentProviderApprovalDecisionV1,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            request_id,
            identity,
            decision,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider resume schema {:?}",
                self.schema
            ));
        }
        validate_line("Agent provider request ID", &self.request_id, 256)?;
        self.identity.validate()?;
        self.decision.validate()?;
        if self.decision.run_identity_digest != self.identity.digest()? {
            return Err("Agent provider approval decision targets another run identity".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderCommandActionV1 {
    Start,
    Cancel,
    Recover,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
// Keep the V1 request fields inline: boxing only `Start` would break the
// published Rust construction API even though serde would preserve the wire
// shape. The primary Node command envelope already boxes this command.
#[allow(clippy::large_enum_variant)]
pub enum AgentProviderCommandV1 {
    Start { request: AgentProviderRunStartV1 },
    Cancel { request: AgentProviderRunCancelV1 },
    Recover { request: AgentProviderRunRecoverV1 },
    Resume { request: AgentProviderRunResumeV1 },
}

impl AgentProviderCommandV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-command.v1";

    pub const fn action(&self) -> AgentProviderCommandActionV1 {
        match self {
            Self::Start { .. } => AgentProviderCommandActionV1::Start,
            Self::Cancel { .. } => AgentProviderCommandActionV1::Cancel,
            Self::Recover { .. } => AgentProviderCommandActionV1::Recover,
            Self::Resume { .. } => AgentProviderCommandActionV1::Resume,
        }
    }

    pub fn identity(&self) -> &AgentProviderRunIdentityV1 {
        match self {
            Self::Start { request } => &request.identity,
            Self::Cancel { request } => &request.identity,
            Self::Recover { request } => &request.identity,
            Self::Resume { request } => &request.identity,
        }
    }

    pub fn request_id(&self) -> &str {
        match self {
            Self::Start { request } => &request.request_id,
            Self::Cancel { request } => &request.request_id,
            Self::Recover { request } => &request.request_id,
            Self::Resume { request } => &request.request_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Start { request } => request.validate(),
            Self::Cancel { request } => request.validate(),
            Self::Recover { request } => request.validate(),
            Self::Resume { request } => request.validate(),
        }
    }

    pub fn validate_for(&self, profile: &AgentProviderProfile) -> Result<(), String> {
        self.validate()?;
        self.identity().validate_for(profile)?;
        if let Self::Start { request } = self {
            if let Some(invocation) = &request.invocation_profile {
                invocation.validate_for(profile)?;
                if invocation.agent.artifact_digest != request.identity.agent_release_identity {
                    return Err(
                        "Harness invocation Agent release does not match its provider run".into(),
                    );
                }
            }
        }
        let required = match self {
            Self::Start { .. } => AgentProviderCapabilityV1::EventPages,
            Self::Cancel { .. } => AgentProviderCapabilityV1::Cancellation,
            Self::Recover { .. } => AgentProviderCapabilityV1::Recovery,
            Self::Resume { .. } => AgentProviderCapabilityV1::PauseResume,
        };
        if !profile.supports(required) {
            return Err(format!(
                "Agent provider command requires unsupported capability {:?}",
                required.as_str()
            ));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode Agent provider command: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderCommandReceiptV1 {
    pub schema: String,
    pub request_id: String,
    pub command_digest: String,
    pub identity: AgentProviderRunIdentityV1,
    pub action: AgentProviderCommandActionV1,
    pub state: AgentProviderRunStateV1,
    pub observed_at_ms: u64,
    pub replayed: bool,
}

impl AgentProviderCommandReceiptV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-command-receipt.v1";

    pub fn accepted(
        profile: &AgentProviderProfile,
        command: &AgentProviderCommandV1,
        state: AgentProviderRunStateV1,
        observed_at_ms: u64,
        replayed: bool,
    ) -> Result<Self, String> {
        command.validate_for(profile)?;
        let receipt = Self {
            schema: Self::SCHEMA.into(),
            request_id: command.request_id().into(),
            command_digest: command.digest()?,
            identity: command.identity().clone(),
            action: command.action(),
            state,
            observed_at_ms,
            replayed,
        };
        receipt.validate_for(profile, command)?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Agent provider command receipt schema {:?}",
                self.schema
            ));
        }
        validate_line("Agent provider receipt request ID", &self.request_id, 256)?;
        validate_digest("Agent provider command digest", &self.command_digest)?;
        self.identity.validate()?;
        if self.observed_at_ms == 0 {
            return Err("Agent provider receipt observation time must be positive".into());
        }
        Ok(())
    }

    pub fn validate_for(
        &self,
        profile: &AgentProviderProfile,
        command: &AgentProviderCommandV1,
    ) -> Result<(), String> {
        self.validate()?;
        command.validate_for(profile)?;
        if self.request_id != command.request_id()
            || self.command_digest != command.digest()?
            || self.identity != *command.identity()
            || self.action != command.action()
        {
            return Err("Agent provider receipt changed its accepted command identity".into());
        }
        if matches!(command, AgentProviderCommandV1::Resume { .. })
            && matches!(
                self.state,
                AgentProviderRunStateV1::Created
                    | AgentProviderRunStateV1::Planning
                    | AgentProviderRunStateV1::AwaitingApproval
            )
        {
            return Err(
                "Agent provider resume receipt did not leave the approval checkpoint".into(),
            );
        }
        Ok(())
    }
}

fn validate_digest(label: &str, value: &str) -> Result<(), String> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} must use canonical lowercase SHA-256 syntax"
        ))
    }
}

fn validate_line(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > max
        || value.contains('\0')
        || value.contains(['\r', '\n'])
    {
        Err(format!(
            "{label} must be a bounded nonempty single-line value"
        ))
    } else {
        Ok(())
    }
}
