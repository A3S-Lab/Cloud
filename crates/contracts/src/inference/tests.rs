use super::*;
use serde_json::json;

fn observed_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-28T00:00:00Z")
        .expect("fixed timestamp")
        .with_timezone(&Utc)
}

fn worker() -> PowerWorkerObservation {
    let observed_at = observed_at();
    PowerWorkerObservation {
        schema: POWER_WORKER_OBSERVATION_SCHEMA.into(),
        worker_epoch: Uuid::from_u128(1),
        observation_generation: 9,
        observed_at,
        expires_at: observed_at + Duration::seconds(15),
        capabilities: PowerWorkerCapabilities {
            phases: vec![InferenceServingPhase::Aggregated],
            prompt_cache: true,
            state_transfer: false,
        },
        ready_phases: vec![InferenceServingPhase::Aggregated],
        admission: PowerAdmissionObservation {
            active_limit: Some(8),
            active: 2,
            waiting: 1,
        },
        prompt_cache: PowerPromptCacheObservation {
            supported: true,
            entries: 2,
            capacity: 8,
            pressure_basis_points: 2_500,
        },
        transfer_health: PowerTransferHealth::Unsupported,
    }
}

#[test]
fn accepts_the_exact_power_v1_contract_at_collection_time() {
    let observation = worker();
    observation
        .validate_at(observed_at() + Duration::seconds(1))
        .expect("valid Power observation");

    let encoded = serde_json::to_value(&observation).expect("encoded observation");
    assert_eq!(encoded["capabilities"]["phases"][0], "aggregated");
    assert_eq!(encoded["prompt_cache"]["pressure_basis_points"], 2_500);
    for forbidden in ["prompt_cache_key", "tenant", "token", "kv_bytes"] {
        assert!(!encoded.to_string().contains(forbidden));
    }
}

#[test]
fn rejects_unknown_fields_and_invalid_power_identity() {
    let mut encoded = serde_json::to_value(worker()).expect("encoded observation");
    encoded["unknown"] = json!(true);
    assert!(serde_json::from_value::<PowerWorkerObservation>(encoded).is_err());

    let mut invalid = worker();
    invalid.worker_epoch = Uuid::nil();
    assert!(invalid
        .validate_at(observed_at() + Duration::seconds(1))
        .is_err());
    invalid = worker();
    invalid.observation_generation = 0;
    assert!(invalid
        .validate_at(observed_at() + Duration::seconds(1))
        .is_err());
}

#[test]
fn rejects_stale_regressed_or_internally_inconsistent_worker_facts() {
    let mut invalid = worker();
    assert!(invalid.validate_at(invalid.expires_at).is_err());

    invalid = worker();
    invalid.ready_phases = vec![InferenceServingPhase::Decode];
    assert!(invalid
        .validate_at(observed_at() + Duration::seconds(1))
        .is_err());

    invalid = worker();
    invalid.admission.active = 9;
    assert!(invalid
        .validate_at(observed_at() + Duration::seconds(1))
        .is_err());

    invalid = worker();
    invalid.prompt_cache.pressure_basis_points = 2_499;
    assert!(invalid
        .validate_at(observed_at() + Duration::seconds(1))
        .is_err());

    invalid = worker();
    invalid.capabilities.state_transfer = true;
    assert!(invalid
        .validate_at(observed_at() + Duration::seconds(1))
        .is_err());
}

#[test]
fn inference_observation_contracts_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<PowerWorkerObservation>();
}
