use super::*;
use async_trait::async_trait;

#[async_trait]
impl IOutboundNotificationSmtpAttemptRepository for PostgresNotificationRepository {
    async fn reserve_smtp_attempt(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpAttemptAdmission, RepositoryError> {
        if !valid_reservation(
            delivery,
            generation,
            fence_token,
            reserved_at,
            lease_expires_at,
        ) {
            return Ok(OutboundNotificationSmtpAttemptAdmission::InvalidFact);
        }
        let delivery = delivery.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(delivery_row) =
                        load_delivery_for_update(transaction, &delivery).await?
                    else {
                        return Ok(OutboundNotificationSmtpAttemptAdmission::InvalidFact);
                    };
                    if !delivery_matches(&delivery_row, &delivery)? {
                        return Ok(OutboundNotificationSmtpAttemptAdmission::InvalidFact);
                    }
                    if let Some(receipt) = decode_delivery_receipt(&delivery_row)? {
                        receipt
                            .validate_against(&delivery)
                            .map_err(PostgresPersistenceError::Invariant)?;
                        return Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(receipt));
                    }

                    if let Some(existing) =
                        load_attempt_for_update(transaction, &delivery, generation).await?
                    {
                        validate_attempt_against_delivery(&existing, &delivery)?;
                        return match existing.state {
                            OutboundNotificationSmtpAttemptState::Reserved => {
                                if reserved_at < existing.lease_expires_at {
                                    if recipient_authority_is_current(transaction, &delivery)
                                        .await?
                                    {
                                        Ok(OutboundNotificationSmtpAttemptAdmission::Deferred {
                                            retry_not_before: existing.lease_expires_at,
                                        })
                                    } else {
                                        let settlement = settle_with_receipt(
                                            transaction,
                                            &delivery,
                                            &existing,
                                            OutboundNotificationSmtpAttemptOutcome::Obsolete,
                                            reserved_at.max(existing.reserved_at),
                                        )
                                        .await?;
                                        Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(
                                            settlement.receipt.ok_or_else(|| {
                                                PostgresPersistenceError::Invariant(
                                                    "obsolete SMTP attempt produced no receipt"
                                                        .into(),
                                                )
                                            })?,
                                        ))
                                    }
                                } else {
                                    let replacement = take_over_reservation(
                                        transaction,
                                        &existing,
                                        fence_token,
                                        reserved_at,
                                        lease_expires_at,
                                    )
                                    .await?;
                                    if recipient_authority_is_current(transaction, &delivery)
                                        .await?
                                    {
                                        Ok(OutboundNotificationSmtpAttemptAdmission::Reserved(
                                            replacement,
                                        ))
                                    } else {
                                        let settlement = settle_with_receipt(
                                            transaction,
                                            &delivery,
                                            &replacement,
                                            OutboundNotificationSmtpAttemptOutcome::Obsolete,
                                            reserved_at,
                                        )
                                        .await?;
                                        Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(
                                            settlement.receipt.ok_or_else(|| {
                                                PostgresPersistenceError::Invariant(
                                                    "obsolete SMTP attempt produced no receipt"
                                                        .into(),
                                                )
                                            })?,
                                        ))
                                    }
                                }
                            }
                            OutboundNotificationSmtpAttemptState::Dispatching => {
                                let deadline = existing.outcome_deadline_at.ok_or_else(|| {
                                    PostgresPersistenceError::Invariant(
                                        "dispatching SMTP attempt has no outcome deadline".into(),
                                    )
                                })?;
                                if reserved_at < deadline {
                                    Ok(OutboundNotificationSmtpAttemptAdmission::Deferred {
                                        retry_not_before: deadline,
                                    })
                                } else {
                                    Ok(OutboundNotificationSmtpAttemptAdmission::Terminal(
                                        recover_indeterminate(transaction, &delivery, &existing)
                                            .await?,
                                    ))
                                }
                            }
                            OutboundNotificationSmtpAttemptState::Terminal => {
                                terminal_admission(&delivery, &delivery_row, existing)
                            }
                        };
                    }

                    validate_prior_generation(transaction, &delivery, generation).await?;
                    let authority_current =
                        recipient_authority_is_current(transaction, &delivery).await?;
                    create_reservation_or_obsolete(
                        transaction,
                        &delivery,
                        generation,
                        fence_token,
                        reserved_at,
                        lease_expires_at,
                        authority_current,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn start_smtp_dispatch(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpDispatchStart, RepositoryError> {
        if !valid_dispatch_start(
            delivery,
            generation,
            fence_token,
            started_at,
            outcome_deadline_at,
        ) {
            return Err(RepositoryError::Storage(
                "outbound SMTP notification dispatch start is invalid".into(),
            ));
        }
        let delivery = delivery.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let delivery_row = load_delivery_for_update(transaction, &delivery)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    if !delivery_matches(&delivery_row, &delivery)? {
                        return Err(RepositoryError::Conflict(
                            "SMTP dispatch delivery fact changed".into(),
                        )
                        .into());
                    }
                    if let Some(receipt) = decode_delivery_receipt(&delivery_row)? {
                        return Ok(OutboundNotificationSmtpDispatchStart::Terminal(receipt));
                    }
                    let existing = load_attempt_for_update(transaction, &delivery, generation)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    validate_attempt_against_delivery(&existing, &delivery)?;
                    match existing.state {
                        OutboundNotificationSmtpAttemptState::Reserved => {
                            if existing.fence_token != fence_token
                                || started_at >= existing.lease_expires_at
                            {
                                return Ok(OutboundNotificationSmtpDispatchStart::Deferred {
                                    retry_not_before: existing.lease_expires_at,
                                });
                            }
                            if !recipient_authority_is_current(transaction, &delivery).await? {
                                let settlement = settle_with_receipt(
                                    transaction,
                                    &delivery,
                                    &existing,
                                    OutboundNotificationSmtpAttemptOutcome::Obsolete,
                                    started_at.max(existing.reserved_at),
                                )
                                .await?;
                                return Ok(OutboundNotificationSmtpDispatchStart::Terminal(
                                    settlement.receipt.ok_or_else(|| {
                                        PostgresPersistenceError::Invariant(
                                            "obsolete SMTP attempt produced no receipt".into(),
                                        )
                                    })?,
                                ));
                            }
                            let dispatching = OutboundNotificationSmtpAttemptRecord::restore(
                                existing.organization_id,
                                existing.delivery_id,
                                existing.recipient_contact_id,
                                existing.generation,
                                existing.attempt_id,
                                OutboundNotificationSmtpAttemptState::Dispatching,
                                None,
                                existing.fence_generation,
                                existing.fence_token,
                                existing.reserved_at,
                                existing.lease_expires_at,
                                Some(started_at),
                                Some(outcome_deadline_at),
                                None,
                            )
                            .map_err(PostgresPersistenceError::Invariant)?;
                            let rows = execute(
                                transaction,
                                sql_query::<()>(
                                    "update notification_outbound_smtp_attempts set state = 'dispatching', dispatch_started_at = ",
                                )
                                .bind(started_at)
                                .append(", outcome_deadline_at = ")
                                .bind(outcome_deadline_at)
                                .append(" where organization_id = ")
                                .bind(existing.organization_id.as_uuid())
                                .append(" and delivery_id = ")
                                .bind(existing.delivery_id)
                                .append(" and generation = ")
                                .bind(existing.generation)
                                .append(" and state = 'reserved' and fence_generation = ")
                                .bind(existing.fence_generation)
                                .append(" and fence_token = ")
                                .bind(fence_token)
                                .append(" and lease_expires_at > ")
                                .bind(started_at),
                            )
                            .await?;
                            if rows != 1 {
                                return Err(PostgresPersistenceError::Invariant(format!(
                                    "starting SMTP notification dispatch affected {rows} rows"
                                )));
                            }
                            Ok(OutboundNotificationSmtpDispatchStart::Authorized(
                                dispatching,
                            ))
                        }
                        OutboundNotificationSmtpAttemptState::Dispatching => {
                            let deadline = existing.outcome_deadline_at.ok_or_else(|| {
                                PostgresPersistenceError::Invariant(
                                    "dispatching SMTP attempt has no outcome deadline".into(),
                                )
                            })?;
                            if started_at < deadline {
                                Ok(OutboundNotificationSmtpDispatchStart::Deferred {
                                    retry_not_before: deadline,
                                })
                            } else {
                                Ok(OutboundNotificationSmtpDispatchStart::Terminal(
                                    recover_indeterminate(transaction, &delivery, &existing)
                                        .await?,
                                ))
                            }
                        }
                        OutboundNotificationSmtpAttemptState::Terminal => {
                            terminal_start(&delivery, &delivery_row, existing)
                        }
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn settle_smtp_attempt(
        &self,
        delivery: &OutboundNotificationDelivery,
        generation: u64,
        fence_token: Uuid,
        outcome: OutboundNotificationSmtpAttemptOutcome,
        settled_at: DateTime<Utc>,
    ) -> Result<OutboundNotificationSmtpAttemptSettlement, RepositoryError> {
        delivery.validate().map_err(RepositoryError::Storage)?;
        if delivery.channel() != OutboundNotificationChannel::Smtp
            || generation == 0
            || generation > delivery.maximum_provider_attempts()
            || fence_token.is_nil()
            || settled_at != canonical_timestamp(settled_at)
            || settled_at < delivery.occurred_at()
        {
            return Err(RepositoryError::Storage(
                "outbound SMTP notification settlement is invalid".into(),
            ));
        }
        let delivery = delivery.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let delivery_row = load_delivery_for_update(transaction, &delivery)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    if !delivery_matches(&delivery_row, &delivery)? {
                        return Err(RepositoryError::Conflict(
                            "SMTP settlement delivery fact changed".into(),
                        )
                        .into());
                    }
                    let existing = load_attempt_for_update(transaction, &delivery, generation)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    validate_attempt_against_delivery(&existing, &delivery)?;

                    if let Some(receipt) = decode_delivery_receipt(&delivery_row)? {
                        let expected = terminal_receipt(&delivery, &existing)?;
                        if expected.as_ref() != Some(&receipt) {
                            return Err(PostgresPersistenceError::Invariant(
                                "SMTP attempt and persisted delivery receipt differ".into(),
                            ));
                        }
                        return Ok(OutboundNotificationSmtpAttemptSettlement {
                            attempt: existing,
                            receipt: Some(receipt),
                        });
                    }
                    if existing.state == OutboundNotificationSmtpAttemptState::Terminal {
                        if existing.fence_token != fence_token || existing.outcome != Some(outcome)
                        {
                            return Err(RepositoryError::Conflict(
                                "terminal SMTP notification attempt differs from its replay".into(),
                            )
                            .into());
                        }
                        return Ok(OutboundNotificationSmtpAttemptSettlement {
                            receipt: terminal_receipt(&delivery, &existing)?,
                            attempt: existing,
                        });
                    }
                    if existing.fence_token != fence_token {
                        return Err(RepositoryError::Conflict(
                            "SMTP notification settlement uses a stale dispatch fence".into(),
                        )
                        .into());
                    }

                    let completed_at =
                        match existing.state {
                            OutboundNotificationSmtpAttemptState::Reserved
                                if outcome == OutboundNotificationSmtpAttemptOutcome::Obsolete =>
                            {
                                settled_at.max(existing.reserved_at)
                            }
                            OutboundNotificationSmtpAttemptState::Dispatching
                                if outcome != OutboundNotificationSmtpAttemptOutcome::Obsolete =>
                            {
                                settled_at.max(existing.dispatch_started_at.ok_or_else(|| {
                                    PostgresPersistenceError::Invariant(
                                        "dispatching SMTP attempt has no start time".into(),
                                    )
                                })?)
                            }
                            _ => return Err(RepositoryError::Conflict(
                                "SMTP notification attempt cannot settle from its current state"
                                    .into(),
                            )
                            .into()),
                        };
                    settle_with_receipt(transaction, &delivery, &existing, outcome, completed_at)
                        .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_smtp_attempt(
        &self,
        organization_id: OrganizationId,
        delivery_id: Uuid,
        generation: u64,
    ) -> Result<Option<OutboundNotificationSmtpAttemptRecord>, RepositoryError> {
        if organization_id.as_uuid().is_nil() || delivery_id.is_nil() || generation == 0 {
            return Err(RepositoryError::Storage(
                "outbound SMTP notification attempt lookup is invalid".into(),
            ));
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<SmtpAttemptRow>(SELECT_SMTP_ATTEMPTS)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and delivery_id = ")
                    .bind(delivery_id)
                    .append(" and generation = ")
                    .bind(generation),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_attempt)
            .transpose()
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }
}
