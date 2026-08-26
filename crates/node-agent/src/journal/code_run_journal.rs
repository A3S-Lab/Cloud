use super::*;
use a3s_cloud_contracts::NodeCodeAgentRuntimeBindingV1;

impl CommandJournal {
    pub(super) fn code_run_bindings_projection(
        &self,
    ) -> Result<BTreeMap<Uuid, NodeCodeAgentRuntimeBindingV1>, CommandJournalError> {
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
                    receipt
                        .validate_for(
                            &binding.profile().map_err(CommandJournalError::Invalid)?,
                            command,
                        )
                        .map_err(CommandJournalError::Invalid)?;
                    if let Ok(code_binding) = binding.code_binding() {
                        bindings.insert(binding.execution_id, code_binding);
                    }
                }
                (
                    NodeCommandPayload::CodeAgentCommand { binding, command },
                    NodeCommandResult::CodeAgentCommandAccepted { receipt },
                ) => {
                    binding
                        .validate_command(command)
                        .map_err(CommandJournalError::Invalid)?;
                    receipt.validate_for(command).map_err(|error| {
                        CommandJournalError::Invalid(format!(
                            "A3S Code command receipt does not match its journaled command ({})",
                            error.code()
                        ))
                    })?;
                    bindings.insert(binding.execution_id, binding.as_ref().clone());
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
    pub async fn code_run_bindings(
        &self,
    ) -> Result<Vec<NodeCodeAgentRuntimeBindingV1>, CommandJournalError> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.code_run_bindings_sync())
            .await
            .map_err(task_error)?
    }

    fn code_run_bindings_sync(
        &self,
    ) -> Result<Vec<NodeCodeAgentRuntimeBindingV1>, CommandJournalError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(JOURNAL_LOCK_FILE))?;
        Ok(self
            .read_journal()?
            .code_run_bindings_projection()?
            .into_values()
            .collect())
    }
}
