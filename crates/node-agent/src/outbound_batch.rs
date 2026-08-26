use serde::{Deserialize, Serialize};

pub(crate) trait OutboundBatchProtocol {
    type Receipt;

    fn validate(&self) -> Result<(), String>;

    fn validate_receipt(&self, receipt: &Self::Receipt) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct DurableOutboundBatch<B>(Option<B>);

impl<B> Default for DurableOutboundBatch<B> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<B> DurableOutboundBatch<B> {
    pub(crate) const fn empty() -> Self {
        Self(None)
    }

    pub(crate) fn pending(&self) -> Option<&B> {
        self.0.as_ref()
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.0.is_some()
    }
}

impl<B> DurableOutboundBatch<B>
where
    B: OutboundBatchProtocol,
{
    pub(crate) fn validate(&self) -> Result<(), OutboundBatchError> {
        if let Some(batch) = self.pending() {
            batch.validate().map_err(OutboundBatchError::Invalid)?;
        }
        Ok(())
    }

    pub(crate) fn stage(&mut self, batch: B) -> Result<(), OutboundBatchError> {
        batch.validate().map_err(OutboundBatchError::Invalid)?;
        if self.is_pending() {
            return Err(OutboundBatchError::Conflict(
                "an outbound batch is already pending".into(),
            ));
        }
        self.0 = Some(batch);
        Ok(())
    }

    pub(crate) fn acknowledge(&mut self, receipt: &B::Receipt) -> Result<B, OutboundBatchError> {
        let pending = self.pending().ok_or_else(|| {
            OutboundBatchError::Conflict("outbound receipt has no pending batch".into())
        })?;
        pending
            .validate_receipt(receipt)
            .map_err(OutboundBatchError::Invalid)?;
        self.0.take().ok_or_else(|| {
            OutboundBatchError::Conflict("outbound batch disappeared during receipt commit".into())
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum OutboundBatchError {
    #[error("invalid outbound batch or receipt: {0}")]
    Invalid(String),
    #[error("outbound batch state conflict: {0}")]
    Conflict(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TestBatch {
        batch_id: Uuid,
        records: u16,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestReceipt {
        batch_id: Uuid,
        accepted_records: u16,
    }

    impl OutboundBatchProtocol for TestBatch {
        type Receipt = TestReceipt;

        fn validate(&self) -> Result<(), String> {
            if self.batch_id.is_nil() || self.records == 0 {
                return Err("test batch identity is invalid".into());
            }
            Ok(())
        }

        fn validate_receipt(&self, receipt: &Self::Receipt) -> Result<(), String> {
            if receipt.batch_id != self.batch_id || receipt.accepted_records != self.records {
                return Err("test receipt does not match its pending batch".into());
            }
            Ok(())
        }
    }

    #[test]
    fn pending_batch_round_trips_as_the_existing_optional_field_shape() {
        #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct State {
            pending: DurableOutboundBatch<TestBatch>,
        }

        let batch = TestBatch {
            batch_id: Uuid::now_v7(),
            records: 2,
        };
        let mut pending = DurableOutboundBatch::empty();
        pending.stage(batch.clone()).expect("stage batch");
        let state = State { pending };
        let value = serde_json::to_value(&state).expect("serialize state");
        assert_eq!(value["pending"]["batch_id"], batch.batch_id.to_string());
        assert_eq!(value["pending"]["records"], 2);

        let restored: State = serde_json::from_value(value).expect("restore state");
        assert_eq!(restored.pending.pending(), Some(&batch));
        restored
            .pending
            .validate()
            .expect("validate restored batch");
    }

    #[test]
    fn only_an_exact_receipt_clears_and_returns_the_pending_batch() {
        let batch = TestBatch {
            batch_id: Uuid::now_v7(),
            records: 3,
        };
        let mut pending = DurableOutboundBatch::empty();
        pending.stage(batch.clone()).expect("stage batch");

        let invalid = TestReceipt {
            batch_id: batch.batch_id,
            accepted_records: 2,
        };
        assert!(matches!(
            pending.acknowledge(&invalid),
            Err(OutboundBatchError::Invalid(_))
        ));
        assert_eq!(pending.pending(), Some(&batch));
        assert!(matches!(
            pending.stage(batch.clone()),
            Err(OutboundBatchError::Conflict(_))
        ));

        let receipt = TestReceipt {
            batch_id: batch.batch_id,
            accepted_records: batch.records,
        };
        assert_eq!(
            pending.acknowledge(&receipt).expect("commit receipt"),
            batch
        );
        assert!(!pending.is_pending());
        assert!(matches!(
            pending.acknowledge(&receipt),
            Err(OutboundBatchError::Conflict(_))
        ));
    }

    #[test]
    fn log_shipper_cannot_reimplement_the_outbound_batch_lifecycle() {
        let source = include_str!("log_shipper.rs");
        assert!(source.contains("DurableOutboundBatch<NodeLogChunkBatch>"));
        for forbidden in [
            "pending: Option<NodeLogChunkBatch>",
            "state.pending = Some(",
            "state.pending = None",
        ] {
            assert!(
                !source.contains(forbidden),
                "log shipping must reuse DurableOutboundBatch; found {forbidden}"
            );
        }
    }

    #[test]
    fn code_event_shipper_reuses_the_same_outbound_batch_lifecycle() {
        let source = include_str!("code_event_shipper.rs");
        assert!(source.contains("DurableOutboundBatch<NodeCodeAgentEventBatchV1>"));
        for forbidden in [
            "pending: Option<NodeCodeAgentEventBatchV1>",
            "state.pending = Some(",
            "state.pending = None",
        ] {
            assert!(
                !source.contains(forbidden),
                "Code event shipping must reuse DurableOutboundBatch; found {forbidden}"
            );
        }
    }

    #[test]
    fn provider_event_shipper_reuses_the_same_outbound_batch_lifecycle() {
        let source = include_str!("agent_provider_event_shipper.rs");
        assert!(source.contains("DurableOutboundBatch<NodeAgentProviderEventBatchV1>"));
        for forbidden in [
            "pending: Option<NodeAgentProviderEventBatchV1>",
            "state.pending = Some(",
            "state.pending = None",
        ] {
            assert!(
                !source.contains(forbidden),
                "provider event shipping must reuse DurableOutboundBatch; found {forbidden}"
            );
        }
    }
}
