use serde::{Deserialize, Serialize};

pub const MAX_GATEWAY_SCOPE_MEMBERS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRolloutPolicy {
    pub min_ready: u32,
    pub max_unavailable: u32,
}

impl GatewayRolloutPolicy {
    pub const fn single_replica() -> Self {
        Self {
            min_ready: 1,
            max_unavailable: 0,
        }
    }

    pub fn new(
        min_ready: u32,
        max_unavailable: u32,
        desired_replicas: usize,
    ) -> Result<Self, String> {
        let policy = Self {
            min_ready,
            max_unavailable,
        };
        policy.validate(desired_replicas)?;
        Ok(policy)
    }

    pub fn validate(self, desired_replicas: usize) -> Result<(), String> {
        if desired_replicas == 0 || desired_replicas > MAX_GATEWAY_SCOPE_MEMBERS {
            return Err(format!(
                "Gateway scope must contain between 1 and {MAX_GATEWAY_SCOPE_MEMBERS} members"
            ));
        }
        let desired_replicas = u32::try_from(desired_replicas)
            .map_err(|_| "Gateway replica count exceeds supported bounds".to_string())?;
        if self.min_ready == 0 || self.min_ready > desired_replicas {
            return Err(
                "Gateway rollout min_ready must be positive and no greater than desired replicas"
                    .into(),
            );
        }
        if self.max_unavailable >= desired_replicas {
            return Err(
                "Gateway rollout max_unavailable must be smaller than desired replicas".into(),
            );
        }
        Ok(())
    }

    pub fn required_ready(self, desired_replicas: usize) -> Result<u32, String> {
        self.validate(desired_replicas)?;
        let desired_replicas = u32::try_from(desired_replicas)
            .map_err(|_| "Gateway replica count exceeds supported bounds".to_string())?;
        Ok(self.min_ready.max(desired_replicas - self.max_unavailable))
    }
}

impl Default for GatewayRolloutPolicy {
    fn default() -> Self {
        Self::single_replica()
    }
}
