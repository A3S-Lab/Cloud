use super::*;

pub(in super::super) async fn record_observations(
    executor: &PostgresExecutor,
    mut batch: NodeObservationBatch,
    received_at: DateTime<Utc>,
) -> Result<NodeObservationReceipt, RepositoryError> {
    batch.sent_at = canonical_timestamp("observation batch send", batch.sent_at)
        .map_err(RepositoryError::Conflict)?;
    batch.heartbeat.observed_at =
        canonical_timestamp("observation heartbeat", batch.heartbeat.observed_at)
            .map_err(RepositoryError::Conflict)?;
    for report in &mut batch.observations {
        report.observed_at = canonical_timestamp("Runtime observation", report.observed_at)
            .map_err(RepositoryError::Conflict)?;
    }
    let received_at = canonical_timestamp("observation receipt", received_at)
        .map_err(RepositoryError::Conflict)?;
    batch.validate().map_err(RepositoryError::Conflict)?;
    let capabilities = NodeCapabilities::new(
        batch.heartbeat.runtime_capabilities.provider_id.to_string(),
        batch.heartbeat.runtime_capabilities.provider_build.clone(),
        serde_json::to_value(&batch.heartbeat.runtime_capabilities)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?,
    )
    .map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                nodes::record_heartbeat_in_transaction(
                    transaction,
                    NodeHeartbeatUpdate {
                        node_id: NodeId::from_uuid(batch.node_id),
                        agent_instance_id: batch.agent_instance_id,
                        agent_version: batch.heartbeat.agent_version.clone(),
                        capabilities,
                        observed_at: batch.heartbeat.observed_at,
                    },
                )
                .await?;

                let mut accepted_reports = 0_u16;
                let mut replayed_reports = 0_u16;
                for report in &batch.observations {
                    let observation = serde_json::to_value(&report.observation)?;
                    if let Some(existing) = fetch_optional::<
                        (Uuid, Option<Uuid>, Uuid, DateTime<Utc>, Value),
                        _,
                    >(
                        transaction,
                        sql_query::<(Uuid, Option<Uuid>, Uuid, DateTime<Utc>, Value)>(
                            "select node_id, command_id, agent_instance_id, observed_at, observation from runtime_observations where report_id = ",
                        )
                        .bind(report.report_id)
                        .append(" for update"),
                    )
                    .await?
                    {
                        if existing
                            != (
                                batch.node_id,
                                report.command_id,
                                batch.agent_instance_id,
                                report.observed_at,
                                observation,
                            )
                        {
                            return Err(RepositoryError::Conflict(
                                "Runtime observation report ID was reused with different content"
                                    .into(),
                            )
                            .into());
                        }
                        replayed_reports = replayed_reports.checked_add(1).ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "observation replay count overflowed".into(),
                            )
                        })?;
                        continue;
                    }
                    if let Some(command_id) = report.command_id {
                        let command_node = fetch_optional::<Uuid, _>(
                            transaction,
                            sql_query::<Uuid>("select node_id from node_commands where id = ")
                                .bind(command_id),
                        )
                        .await?
                        .ok_or_else(|| {
                            RepositoryError::Conflict(
                                "Runtime observation references an unknown command".into(),
                            )
                        })?;
                        if command_node != batch.node_id {
                            return Err(RepositoryError::Conflict(
                                "Runtime observation command belongs to another node".into(),
                            )
                            .into());
                        }
                    }
                    require_one_row(
                        "Runtime observation",
                        execute(
                            transaction,
                            sql_query::<()>(
                                "insert into runtime_observations (report_id, node_id, command_id, agent_instance_id, observed_at, received_at, unit_id, generation, observation) values (",
                            )
                            .bind(report.report_id)
                            .append(", ")
                            .bind(batch.node_id)
                            .append(", ")
                            .bind(report.command_id)
                            .append(", ")
                            .bind(batch.agent_instance_id)
                            .append(", ")
                            .bind(report.observed_at)
                            .append(", ")
                            .bind(received_at)
                            .append(", ")
                            .bind(report.observation.unit_id.as_str())
                            .append(", ")
                            .bind(report.observation.generation)
                            .append(", ")
                            .bind(serde_json::to_value(&report.observation)?)
                            .append(")"),
                        )
                        .await?,
                    )?;
                    accepted_reports = accepted_reports.checked_add(1).ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "observation acceptance count overflowed".into(),
                        )
                    })?;
                }
                let receipt = NodeObservationReceipt {
                    schema: NodeObservationReceipt::SCHEMA.into(),
                    node_id: batch.node_id,
                    heartbeat_observed_at: batch.heartbeat.observed_at,
                    accepted_reports,
                    replayed_reports,
                };
                receipt
                    .validate()
                    .map_err(PostgresPersistenceError::Invariant)?;
                Ok(receipt)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn latest_runtime_observation(
    executor: &PostgresExecutor,
    node_id: NodeId,
    unit_id: &str,
    generation: u64,
) -> Result<Option<RuntimeObservationRecord>, RepositoryError> {
    if unit_id.is_empty() || unit_id.len() > 512 || unit_id.contains('\0') || generation == 0 {
        return Err(RepositoryError::Conflict(
            "Runtime observation lookup identity is invalid".into(),
        ));
    }
    let row = a3s_orm::Database::new(a3s_orm::PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<(Uuid, Uuid, Option<Uuid>, DateTime<Utc>, DateTime<Utc>, Value)>(
                "select report_id, node_id, command_id, observed_at, received_at, observation from runtime_observations where node_id = ",
            )
            .bind(node_id.as_uuid())
            .append(" and unit_id = ")
            .bind(unit_id)
            .append(" and generation = ")
            .bind(generation)
            .append(" order by observed_at desc, report_id desc limit 1"),
        )
        .await
        .map_err(|error| RepositoryError::Storage(error.to_string()))?;
    row.map(
        |(report_id, stored_node_id, command_id, observed_at, received_at, document)| {
            let observation: a3s_runtime::contract::RuntimeObservation =
                serde_json::from_value(document).map_err(|error| {
                    RepositoryError::Storage(format!(
                        "stored Runtime observation is invalid: {error}"
                    ))
                })?;
            observation.validate().map_err(|error| {
                RepositoryError::Storage(format!("stored Runtime observation is invalid: {error}"))
            })?;
            if stored_node_id != node_id.as_uuid()
                || observation.unit_id != unit_id
                || observation.generation != generation
            {
                return Err(RepositoryError::Storage(
                    "stored Runtime observation identity is inconsistent".into(),
                ));
            }
            Ok(RuntimeObservationRecord {
                report_id,
                node_id,
                command_id: command_id.map(NodeCommandId::from_uuid),
                observed_at,
                received_at,
                observation,
            })
        },
    )
    .transpose()
}
