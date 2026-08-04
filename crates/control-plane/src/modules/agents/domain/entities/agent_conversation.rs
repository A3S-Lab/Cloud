use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentConversationId, EnvironmentId, OrganizationId, ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationStatus {
    Active,
    Closed,
}

impl AgentConversationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Closed => "closed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "closed" => Ok(Self::Closed),
            _ => Err(format!("unsupported Agent conversation status {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: AgentConversationId,
    pub status: AgentConversationStatus,
    pub last_event_sequence: u64,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl AgentConversation {
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        id: AgentConversationId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let conversation = Self {
            organization_id,
            project_id,
            environment_id,
            id,
            status: AgentConversationStatus::Active,
            last_event_sequence: 0,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            closed_at: None,
        };
        conversation.validate()?;
        Ok(conversation)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.created_at = canonical_timestamp(self.created_at);
        self.updated_at = canonical_timestamp(self.updated_at);
        self.closed_at = self.closed_at.map(canonical_timestamp);
        self.validate()?;
        Ok(self)
    }

    pub fn allocate_event_sequences(
        &mut self,
        count: usize,
        occurred_at: DateTime<Utc>,
    ) -> Result<u64, String> {
        if self.status != AgentConversationStatus::Active {
            return Err("closed Agent conversation cannot append events".into());
        }
        let count = u64::try_from(count)
            .map_err(|_| "Agent event batch exceeds the supported sequence range".to_owned())?;
        if count == 0 {
            return Err("Agent event batch must not be empty".into());
        }
        let first_sequence = self
            .last_event_sequence
            .checked_add(1)
            .ok_or_else(|| "Agent conversation event sequence overflowed".to_owned())?;
        let last_event_sequence = self
            .last_event_sequence
            .checked_add(count)
            .ok_or_else(|| "Agent conversation event sequence overflowed".to_owned())?;
        let updated_at = std::cmp::max(self.updated_at, canonical_timestamp(occurred_at));
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Agent conversation aggregate version overflowed".to_owned())?;
        self.last_event_sequence = last_event_sequence;
        self.updated_at = updated_at;
        self.aggregate_version = aggregate_version;
        Ok(first_sequence)
    }

    pub fn close(&mut self, closed_at: DateTime<Utc>) -> Result<(), String> {
        if self.status == AgentConversationStatus::Closed {
            return self.observe_time(closed_at);
        }
        let closed_at = self.validated_time(closed_at)?;
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Agent conversation aggregate version overflowed".to_owned())?;
        self.status = AgentConversationStatus::Closed;
        self.updated_at = closed_at;
        self.closed_at = Some(closed_at);
        self.aggregate_version = aggregate_version;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || self
                .closed_at
                .is_some_and(|closed_at| closed_at != canonical_timestamp(closed_at))
            || (self.status == AgentConversationStatus::Closed) != self.closed_at.is_some()
            || self.closed_at.is_some_and(|closed_at| {
                closed_at < self.created_at || closed_at != self.updated_at
            })
        {
            return Err("Agent conversation aggregate is invalid".into());
        }
        Ok(())
    }

    fn observe_time(&mut self, occurred_at: DateTime<Utc>) -> Result<(), String> {
        let occurred_at = self.validated_time(occurred_at)?;
        self.updated_at = occurred_at;
        Ok(())
    }

    fn validated_time(&self, occurred_at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        let occurred_at = canonical_timestamp(occurred_at);
        if occurred_at < self.updated_at {
            return Err("Agent conversation transition time regressed".into());
        }
        Ok(occurred_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_contiguous_sequences_and_closes_the_stream() {
        let at = Utc::now();
        let mut conversation = AgentConversation::create(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            AgentConversationId::new(),
            at,
        )
        .expect("conversation");

        assert_eq!(
            conversation.allocate_event_sequences(2, at).expect("batch"),
            1
        );
        assert_eq!(conversation.last_event_sequence, 2);
        assert_eq!(
            conversation
                .allocate_event_sequences(1, at)
                .expect("suffix"),
            3
        );
        conversation.close(at).expect("close");
        assert!(conversation.allocate_event_sequences(1, at).is_err());
    }

    #[test]
    fn delayed_event_time_does_not_override_the_authoritative_sequence_order() {
        let at = Utc::now();
        let mut conversation = AgentConversation::create(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            AgentConversationId::new(),
            at,
        )
        .expect("conversation");
        let later = at + chrono::Duration::seconds(2);
        let delayed = at + chrono::Duration::seconds(1);

        assert_eq!(
            conversation
                .allocate_event_sequences(1, later)
                .expect("first event"),
            1
        );
        assert_eq!(
            conversation
                .allocate_event_sequences(1, delayed)
                .expect("delayed event"),
            2
        );
        assert_eq!(conversation.last_event_sequence, 2);
        assert_eq!(conversation.updated_at, canonical_timestamp(later));

        let before_close = conversation.clone();
        assert!(conversation.close(delayed).is_err());
        assert_eq!(conversation, before_close);
    }
}
