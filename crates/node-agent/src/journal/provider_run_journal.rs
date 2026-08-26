use super::*;
use a3s_cloud_contracts::{NodeAgentProviderRuntimeBindingV1, NATIVE_CODE_AGENT_PROVIDER_KIND};

impl CommandJournal {
    pub(super) fn provider_run_bindings_projection(
        &self,
    ) -> Result<BTreeMap<Uuid, NodeAgentProviderRuntimeBindingV1>, CommandJournalError> {
        let mut bindings = BTreeMap::new();
        let mut entries = self.entries.values().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.envelope.sequence);
        for entry in entries {
            let Some(JournalCompletion {
                outcome: NodeCommandOutcome::Succeeded { result },
                ..
            }) = entry.completion.as_ref()
            else {
                continue;
            };
            match (&entry.envelope.payload, result.as_ref()) {
                (
                    NodeCommandPayload::AgentProviderCommand { binding, command },
                    NodeCommandResult::AgentProviderCommandAccepted { receipt },
                ) => {
                    binding
                        .validate_command(command)
                        .map_err(CommandJournalError::Invalid)?;
                    let profile = binding.profile().map_err(CommandJournalError::Invalid)?;
                    receipt
                        .validate_for(&profile, command)
                        .map_err(CommandJournalError::Invalid)?;
                    if profile.kind() != NATIVE_CODE_AGENT_PROVIDER_KIND {
                        match bindings.insert(binding.execution_id, binding.as_ref().clone()) {
                            Some(existing) if existing != **binding => {
                                return Err(CommandJournalError::Conflict(
                                    "one Agent execution changed its provider Runtime binding"
                                        .into(),
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                (
                    NodeCommandPayload::RuntimeRemove { request },
                    NodeCommandResult::RuntimeRemoved { removal },
                ) if removal.unit_id == request.unit_id
                    && removal.generation == request.generation =>
                {
                    bindings.retain(|_, binding| {
                        binding.runtime_unit_id != request.unit_id
                            || binding.runtime_generation != request.generation
                    });
                }
                _ => {}
            }
        }
        Ok(bindings)
    }
}

impl FileCommandJournal {
    pub async fn provider_run_bindings(
        &self,
    ) -> Result<Vec<NodeAgentProviderRuntimeBindingV1>, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.provider_run_bindings_sync())
            .await
            .map_err(task_error)?
    }

    fn provider_run_bindings_sync(
        &self,
    ) -> Result<Vec<NodeAgentProviderRuntimeBindingV1>, CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        Ok(self
            .read_journal()?
            .provider_run_bindings_projection()?
            .into_values()
            .collect())
    }
}
