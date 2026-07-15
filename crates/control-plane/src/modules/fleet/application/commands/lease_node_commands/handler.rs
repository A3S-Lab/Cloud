use super::LeaseNodeCommands;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use uuid::Uuid;

pub struct LeaseNodeCommandsHandler {
    commands: Arc<dyn INodeControlRepository>,
    lease_duration: Duration,
    maximum_wait: StdDuration,
    retry_interval: StdDuration,
}

impl LeaseNodeCommandsHandler {
    pub fn new(
        commands: Arc<dyn INodeControlRepository>,
        lease_duration: Duration,
        maximum_wait: StdDuration,
        retry_interval: StdDuration,
    ) -> Result<Self, String> {
        if lease_duration <= Duration::zero()
            || maximum_wait.is_zero()
            || retry_interval.is_zero()
            || retry_interval > maximum_wait
        {
            return Err("node command polling policy is invalid".into());
        }
        Ok(Self {
            commands,
            lease_duration,
            maximum_wait,
            retry_interval,
        })
    }
}

impl CommandHandler<LeaseNodeCommands> for LeaseNodeCommandsHandler {
    fn execute(
        &self,
        command: LeaseNodeCommands,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<a3s_cloud_contracts::NodeCommandLeaseResponse>>,
    > {
        let commands = Arc::clone(&self.commands);
        let lease_duration = self.lease_duration;
        let maximum_wait = self.maximum_wait;
        let retry_interval = self.retry_interval;
        Box::pin(async move {
            if let Err(error) = command.request.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            if command.request.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the requested node".into(),
                )));
            }
            let requested_wait = StdDuration::from_millis(command.request.wait_ms);
            if requested_wait > maximum_wait {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "command long poll exceeds the configured {} ms limit",
                    maximum_wait.as_millis()
                ))));
            }
            let deadline = tokio::time::Instant::now() + requested_wait;
            let mut now = command.received_at;
            loop {
                let leased_until = checked_add(now, lease_duration)?;
                let response = match commands
                    .lease_commands(&command.request, Uuid::now_v7(), now, leased_until)
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error.into())),
                };
                if !response.commands.is_empty()
                    || requested_wait.is_zero()
                    || tokio::time::Instant::now() >= deadline
                {
                    return Ok(Ok(response));
                }
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                tokio::time::sleep(retry_interval.min(remaining)).await;
                now = Utc::now();
            }
        })
    }
}

fn checked_add(now: DateTime<Utc>, duration: Duration) -> a3s_boot::Result<DateTime<Utc>> {
    now.checked_add_signed(duration)
        .ok_or_else(|| a3s_boot::BootError::Internal("command lease expiry overflowed".into()))
}
