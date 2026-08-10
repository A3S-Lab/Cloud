use super::*;
use std::time::Duration;

pub(super) async fn exercise_resume_delivery_leases(
    repository: &PostgresHumanTaskRepository,
    authorities: Authorities,
    decision_record: &HumanTaskDecisionRecord,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let first_owner = Uuid::now_v7();
    let first = repository
        .claim_resume_deliveries(first_owner, 10, timestamp(8, 6), Duration::from_secs(60))
        .await
        .map_err(|error| std::io::Error::other(format!("first resume lease claim: {error:?}")))?;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].record, *decision_record);
    assert_eq!(first[0].attempt_count, 1);
    assert_eq!(first[0].lease_owner, first_owner);
    assert_eq!(first[0].claimed_at, timestamp(8, 6));
    assert_eq!(first[0].lease_expires_at, timestamp(8, 7));

    let second_owner = Uuid::now_v7();
    assert!(repository
        .claim_resume_deliveries(
            second_owner,
            10,
            timestamp(8, 6) + chrono::Duration::seconds(30),
            Duration::from_secs(60),
        )
        .await?
        .is_empty());

    let reclaimed = repository
        .claim_resume_deliveries(second_owner, 10, timestamp(8, 7), Duration::from_secs(60))
        .await?;
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].attempt_count, 2);
    assert_eq!(reclaimed[0].lease_owner, second_owner);

    assert!(matches!(
        repository
            .retry_resume_delivery(
                authorities.organization_id,
                decision_record.decision.id,
                first_owner,
                "stale worker",
                timestamp(8, 7) + chrono::Duration::seconds(5),
                Duration::from_secs(30),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    repository
        .retry_resume_delivery(
            authorities.organization_id,
            decision_record.decision.id,
            second_owner,
            "temporary Flow store outage",
            timestamp(8, 7) + chrono::Duration::seconds(10),
            Duration::from_secs(30),
        )
        .await?;

    assert!(repository
        .claim_resume_deliveries(
            Uuid::now_v7(),
            10,
            timestamp(8, 7) + chrono::Duration::seconds(20),
            Duration::from_secs(60),
        )
        .await?
        .is_empty());

    let final_owner = Uuid::now_v7();
    let final_claim = repository
        .claim_resume_deliveries(
            final_owner,
            10,
            timestamp(8, 7) + chrono::Duration::seconds(40),
            Duration::from_secs(5 * 60),
        )
        .await?;
    assert_eq!(final_claim.len(), 1);
    assert_eq!(final_claim[0].attempt_count, 3);
    assert_eq!(final_claim[0].lease_owner, final_owner);

    assert!(matches!(
        repository
            .conflict_resume_delivery(
                authorities.organization_id,
                decision_record.decision.id,
                second_owner,
                "stale payload conflict",
                timestamp(8, 8),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    Ok(final_owner)
}
