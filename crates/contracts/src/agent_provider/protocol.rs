use super::{AgentProviderCapabilityV1, AgentProviderProfile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_PROVIDER_PROMPT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderRunStateV1 {
    Created,
    Planning,
    Executing,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

impl AgentProviderRunStateV1 {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProviderRunStartV1 {
    pub schema: String,
    pub request_id: String,
    pub identity: AgentProviderRunIdentityV1,
    pub prompt: String,
}

impl AgentProviderRunStartV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-run-start.v1";

    pub fn new(
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        prompt: String,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            request_id,
            identity,
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
        if self.prompt.trim().is_empty()
            || self.prompt.len() > MAX_PROVIDER_PROMPT_BYTES
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderCommandActionV1 {
    Start,
    Cancel,
    Recover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentProviderCommandV1 {
    Start { request: AgentProviderRunStartV1 },
    Cancel { request: AgentProviderRunCancelV1 },
    Recover { request: AgentProviderRunRecoverV1 },
}

impl AgentProviderCommandV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.agent-provider-command.v1";

    pub const fn action(&self) -> AgentProviderCommandActionV1 {
        match self {
            Self::Start { .. } => AgentProviderCommandActionV1::Start,
            Self::Cancel { .. } => AgentProviderCommandActionV1::Cancel,
            Self::Recover { .. } => AgentProviderCommandActionV1::Recover,
        }
    }

    pub fn identity(&self) -> &AgentProviderRunIdentityV1 {
        match self {
            Self::Start { request } => &request.identity,
            Self::Cancel { request } => &request.identity,
            Self::Recover { request } => &request.identity,
        }
    }

    pub fn request_id(&self) -> &str {
        match self {
            Self::Start { request } => &request.request_id,
            Self::Cancel { request } => &request.request_id,
            Self::Recover { request } => &request.request_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Start { request } => request.validate(),
            Self::Cancel { request } => request.validate(),
            Self::Recover { request } => request.validate(),
        }
    }

    pub fn validate_for(&self, profile: &AgentProviderProfile) -> Result<(), String> {
        self.validate()?;
        self.identity().validate_for(profile)?;
        let required = match self {
            Self::Start { .. } => AgentProviderCapabilityV1::EventPages,
            Self::Cancel { .. } => AgentProviderCapabilityV1::Cancellation,
            Self::Recover { .. } => AgentProviderCapabilityV1::Recovery,
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
