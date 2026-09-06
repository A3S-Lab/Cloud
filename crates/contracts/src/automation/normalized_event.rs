use super::invocation::AutomationInvocationInputV1;
use super::validation::{
    canonical_json, json_digest, validate_digest, validate_event_key, validate_timestamp,
    validate_uuid,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const AUTOMATION_NORMALIZED_EVENT_SCHEMA_V1: &str = "cloud.automation.normalized-event.v1";
pub const AUTOMATION_NORMALIZED_EVENT_MAX_BYTES: usize = 2 * 1024 * 1024;

/// One provider-neutral event fact emitted by a subscription/source owner.
///
/// The event digest binds the canonical identity, observation time, and bounded
/// normalized payload or object reference. Automations validates that binding
/// without interpreting provider-specific fields or proving provider authenticity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationNormalizedEventV1 {
    pub schema: String,
    pub event_id: Uuid,
    pub event_key: String,
    pub event_digest: String,
    pub observed_at: DateTime<Utc>,
    pub input: AutomationInvocationInputV1,
}

impl AutomationNormalizedEventV1 {
    pub const SCHEMA: &'static str = AUTOMATION_NORMALIZED_EVENT_SCHEMA_V1;

    pub fn new(
        event_id: Uuid,
        event_key: impl Into<String>,
        observed_at: DateTime<Utc>,
        input: AutomationInvocationInputV1,
    ) -> Result<Self, String> {
        let mut event = Self {
            schema: Self::SCHEMA.into(),
            event_id,
            event_key: event_key.into(),
            event_digest: String::new(),
            observed_at,
            input,
        };
        event.event_digest = event.computed_digest()?;
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Automation normalized event schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation normalized event ID", self.event_id)?;
        validate_event_key(&self.event_key)?;
        validate_digest("Automation normalized event digest", &self.event_digest)?;
        validate_timestamp("Automation normalized event observed_at", self.observed_at)?;
        self.input.validate()?;
        canonical_json(self, AUTOMATION_NORMALIZED_EVENT_MAX_BYTES)?;
        if self.event_digest != self.computed_digest()? {
            return Err("Automation normalized event content and digest do not match".into());
        }
        Ok(())
    }

    fn computed_digest(&self) -> Result<String, String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DigestMaterial<'a> {
            schema: &'a str,
            event_id: Uuid,
            event_key: &'a str,
            observed_at: DateTime<Utc>,
            input: &'a AutomationInvocationInputV1,
        }

        Ok(json_digest(&canonical_json(
            &DigestMaterial {
                schema: &self.schema,
                event_id: self.event_id,
                event_key: &self.event_key,
                observed_at: self.observed_at,
                input: &self.input,
            },
            AUTOMATION_NORMALIZED_EVENT_MAX_BYTES,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input() -> AutomationInvocationInputV1 {
        AutomationInvocationInputV1::inline_json(json!({"source": "plugin"})).expect("input")
    }

    fn event() -> AutomationNormalizedEventV1 {
        AutomationNormalizedEventV1::new(
            Uuid::from_u128(0x018f0000000070008000000000000420),
            "plugin.package.updated",
            DateTime::from_timestamp(1_767_229_200, 0).expect("timestamp"),
            input(),
        )
        .expect("event")
    }

    #[test]
    fn constructs_and_validates_a_provider_neutral_event_fact() {
        let value = event();
        assert_eq!(value.schema, AutomationNormalizedEventV1::SCHEMA);
        assert!(value.event_digest.starts_with("sha256:"));
        assert_eq!(value.event_digest, event().event_digest);
        value.validate().expect("normalized event");
    }

    #[test]
    fn rejects_schema_identity_event_key_digest_and_input_drift() {
        let mut invalid = event();
        invalid.schema = "cloud.other.v1".into();
        assert!(invalid.validate().is_err());

        let mut invalid = event();
        invalid.event_key = "Plugin.Package.Updated".into();
        assert!(invalid.validate().is_err());

        let mut invalid = event();
        invalid.event_digest = "sha256:not-a-digest".into();
        assert!(invalid.validate().is_err());

        let mut invalid = event();
        invalid.input = AutomationInvocationInputV1::InlineJson {
            value: json!({"source": "changed"}),
            digest: format!("sha256:{}", "b".repeat(64)),
            size_bytes: 1,
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn rejects_otherwise_valid_identity_time_and_payload_substitution() {
        let mut changed = event();
        changed.event_id = Uuid::from_u128(0x018f0000000070008000000000000421);
        assert!(changed.validate().is_err());

        let mut changed = event();
        changed.event_key = "plugin.package.deleted".into();
        assert!(changed.validate().is_err());

        let mut changed = event();
        changed.observed_at = DateTime::from_timestamp(1_767_229_201, 0).expect("timestamp");
        assert!(changed.validate().is_err());

        let mut changed = event();
        changed.input = AutomationInvocationInputV1::inline_json(json!({"source": "changed"}))
            .expect("valid changed input");
        assert!(changed.validate().is_err());
    }

    #[test]
    fn json_round_trip_retains_the_digest_and_rejects_unknown_fields() {
        let value = event();
        let mut json = serde_json::to_value(&value).expect("JSON");
        let restored: AutomationNormalizedEventV1 =
            serde_json::from_value(json.clone()).expect("restored event");
        restored.validate().expect("restored binding");
        assert_eq!(restored, value);
        json["providerCredential"] = json!("unexpected");
        assert!(serde_json::from_value::<AutomationNormalizedEventV1>(json).is_err());
    }
}
