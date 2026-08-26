use crate::modules::agents::domain::AgentProviderProfileBinding;
use a3s_cloud_contracts::{
    AgentProviderCapabilityNegotiationV1, AgentProviderCapabilityRequirementsV1,
    AgentProviderCommandV1, AgentProviderRunCancelV1, AgentProviderRunIdentityV1,
    AgentProviderRunRecoverV1, AgentProviderRunStartV1, HarnessInvocationProfileV1,
};

/// Sole provider-neutral admission port for an Agent execution lifecycle.
///
/// Implementations may translate this contract to a private Harness protocol,
/// but cannot invent another Cloud execution, scheduler, queue, or event log.
pub trait AgentExecutionProvider: Send + Sync {
    fn profile(&self) -> &AgentProviderProfileBinding;

    fn negotiate(
        &self,
        requirements: &AgentProviderCapabilityRequirementsV1,
    ) -> Result<AgentProviderCapabilityNegotiationV1, String> {
        self.profile().profile()?.negotiate(requirements)
    }

    /// Builds the pre-A1.4 profile-less command retained only for protocol
    /// conformance and durable legacy-history compatibility.
    fn start_command(
        &self,
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        prompt: String,
    ) -> Result<AgentProviderCommandV1, String> {
        let command = AgentProviderCommandV1::Start {
            request: AgentProviderRunStartV1::new(request_id, identity, prompt)?,
        };
        command.validate_for(&self.profile().profile()?)?;
        Ok(command)
    }

    /// Sole start constructor used by the production Flow dispatch path.
    fn start_command_with_invocation_profile(
        &self,
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        invocation_profile: HarnessInvocationProfileV1,
        prompt: String,
    ) -> Result<AgentProviderCommandV1, String> {
        let command = AgentProviderCommandV1::Start {
            request: AgentProviderRunStartV1::new_with_invocation_profile(
                request_id,
                identity,
                invocation_profile,
                prompt,
            )?,
        };
        command.validate_for(&self.profile().profile()?)?;
        Ok(command)
    }

    fn cancel_command(
        &self,
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        reason: String,
    ) -> Result<AgentProviderCommandV1, String> {
        let command = AgentProviderCommandV1::Cancel {
            request: AgentProviderRunCancelV1::new(request_id, identity, reason)?,
        };
        command.validate_for(&self.profile().profile()?)?;
        Ok(command)
    }

    fn recover_command(
        &self,
        request_id: String,
        identity: AgentProviderRunIdentityV1,
        checkpoint_run_id: String,
    ) -> Result<AgentProviderCommandV1, String> {
        let command = AgentProviderCommandV1::Recover {
            request: AgentProviderRunRecoverV1::new(request_id, identity, checkpoint_run_id)?,
        };
        command.validate_for(&self.profile().profile()?)?;
        Ok(command)
    }
}
