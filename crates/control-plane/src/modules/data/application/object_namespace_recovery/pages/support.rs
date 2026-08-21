use super::*;

impl ObjectNamespaceRecoveryExecutor {
    pub(super) fn validate_seal_request(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        previous: Option<&ObjectNamespaceRecoveryPoint>,
        writer_epoch: u64,
        writer_fence_receipt_digest: &Sha256Digest,
        sealed_at: DateTime<Utc>,
    ) -> Result<(u64, Option<Sha256Digest>), ObjectNamespaceError> {
        self.validate_source_recovery_binding(source, recovery)?;
        if writer_epoch == 0 {
            return Err(invalid("object namespace writer epoch must be positive"));
        }
        canonical_digest(writer_fence_receipt_digest).map_err(ObjectNamespaceError::Invalid)?;
        let sealed_at = canonical_timestamp(sealed_at);
        match previous {
            Some(point) => {
                point.validate().map_err(ObjectNamespaceError::Invalid)?;
                if point.spec().namespace_id != source.namespace_id
                    || point.spec().provider_profile_digest != source.provider_profile_digest
                    || writer_epoch < point.spec().writer_epoch
                    || sealed_at < point.spec().sealed_at
                {
                    return Err(invalid(
                        "object namespace recovery predecessor changed or regressed",
                    ));
                }
                Ok((
                    point.spec().sequence.checked_add(1).ok_or_else(|| {
                        invalid("object namespace recovery sequence is exhausted")
                    })?,
                    Some(point.digest().clone()),
                ))
            }
            None => Ok((1, None)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn validate_delete_request(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
    ) -> Result<(), ObjectNamespaceError> {
        deletion_plan
            .validate_against(point, restore_plan, restore_evidence, retention_policy)
            .map_err(ObjectNamespaceError::Invalid)?;
        self.validate_deletion_bindings(
            source,
            recovery,
            retained,
            point,
            restore_plan,
            deletion_plan,
        )
    }

    pub(super) async fn list_live_entries(
        &self,
        access: &ObjectNamespaceAccess,
    ) -> Result<Vec<ObjectNamespaceEntry>, ObjectNamespaceError> {
        let mut listed = access
            .namespace
            .list(None, self.maximum_objects, self.maximum_state_bytes)
            .await?;
        listed.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        if listed
            .iter()
            .any(|entry| entry.size_bytes > self.maximum_object_bytes)
        {
            return Err(corrupt(
                "object namespace page contains an object beyond its admission bound",
            ));
        }
        Ok(listed)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn observe_manifest_page(
        &self,
        access: &ObjectNamespaceAccess,
        manifest: &RecoveryManifest,
        phase: &str,
        binding_digest: &Sha256Digest,
        require_exact: bool,
        page_index: u32,
        previous: Option<&ObjectNamespaceObservationPageCheckpoint>,
    ) -> Result<ObjectNamespaceObservationPageCheckpoint, ObjectNamespaceError> {
        self.validate_next_observation_page(phase, binding_digest, page_index, previous)?;
        let listed = self.list_live_entries(access).await?;
        if require_exact {
            require_metadata_complete(&listed, &manifest.entries)?;
        } else {
            require_metadata_subset(&listed, &manifest.entries)?;
        }
        let start_after = previous.and_then(|checkpoint| checkpoint.last_key.clone());
        let remaining = entries_after(&listed, start_after.as_ref());
        let page_end = self.page_end_for_listed(remaining)?;
        let selected = &remaining[..page_end];
        let mut observed = Vec::with_capacity(selected.len());
        for listed_entry in selected {
            let body = read_exact(
                access.namespace.as_ref(),
                &listed_entry.key,
                self.maximum_object_bytes,
            )
            .await?;
            if body.len() as u64 != listed_entry.size_bytes {
                return Err(corrupt(
                    "object namespace changed between checkpoint listing and read",
                ));
            }
            observed.push(RecoveryManifestEntry {
                key: listed_entry.key.clone(),
                digest: Sha256Digest::from_bytes(&body),
                size_bytes: listed_entry.size_bytes,
            });
        }
        require_subset(&observed, &manifest.entries)?;
        self.build_observation_checkpoint(
            phase,
            binding_digest,
            page_index,
            start_after,
            &observed,
            page_end == remaining.len(),
            previous,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_observation_checkpoint(
        &self,
        phase: &str,
        binding_digest: &Sha256Digest,
        page_index: u32,
        start_after: Option<ObjectNamespaceKey>,
        entries: &[RecoveryManifestEntry],
        complete: bool,
        previous: Option<&ObjectNamespaceObservationPageCheckpoint>,
    ) -> Result<ObjectNamespaceObservationPageCheckpoint, ObjectNamespaceError> {
        let page_bytes = state_size(entries)?;
        let processed_objects = previous
            .map_or(Ok(0_u32), |checkpoint| Ok(checkpoint.processed_objects))?
            .checked_add(entries.len() as u32)
            .ok_or_else(|| corrupt("object namespace observation count overflowed"))?;
        let processed_bytes = previous
            .map_or(Ok(0_u64), |checkpoint| Ok(checkpoint.processed_bytes))?
            .checked_add(page_bytes)
            .ok_or_else(|| corrupt("object namespace observation size overflowed"))?;
        let last_key = entries
            .last()
            .map(|entry| entry.key.clone())
            .or_else(|| start_after.clone());
        let digest = self.checkpoint_digest(
            "object namespace observation page checkpoint",
            &ObservationCheckpointProjection {
                phase,
                binding_digest,
                page_index,
                start_after: &start_after,
                last_key: &last_key,
                processed_objects,
                processed_bytes,
                complete,
                previous_checkpoint_digest: previous
                    .map(|checkpoint| &checkpoint.checkpoint_digest),
                page_entries: entries,
            },
        )?;
        Ok(ObjectNamespaceObservationPageCheckpoint {
            phase: phase.into(),
            binding_digest: binding_digest.clone(),
            page_index,
            start_after,
            last_key,
            processed_objects,
            processed_bytes,
            complete,
            checkpoint_digest: digest,
        })
    }

    pub(super) fn manifest_page_range(
        &self,
        phase: &str,
        binding_digest: &Sha256Digest,
        manifest: &RecoveryManifest,
        page_index: u32,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<(usize, usize), ObjectNamespaceError> {
        self.validate_next_manifest_page(
            phase,
            binding_digest,
            &Sha256Digest::from_bytes(&manifest_bytes(manifest, self.maximum_manifest_bytes)?),
            page_index,
            previous,
        )?;
        let start = previous.map_or(0, |checkpoint| checkpoint.next_index as usize);
        if start >= manifest.entries.len() {
            return Err(corrupt(
                "object namespace manifest page cursor is exhausted",
            ));
        }
        let relative_end = self.page_end_for_manifest(&manifest.entries[start..])?;
        Ok((start, start + relative_end))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_manifest_checkpoint(
        &self,
        phase: &str,
        binding_digest: &Sha256Digest,
        manifest: &RecoveryManifest,
        page_index: u32,
        start: usize,
        end: usize,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<ObjectNamespaceManifestPageCheckpoint, ObjectNamespaceError> {
        let manifest_digest =
            Sha256Digest::from_bytes(&manifest_bytes(manifest, self.maximum_manifest_bytes)?);
        let processed_bytes = previous
            .map_or(0, |checkpoint| checkpoint.processed_bytes)
            .checked_add(state_size(&manifest.entries[start..end])?)
            .ok_or_else(|| corrupt("object namespace manifest checkpoint size overflowed"))?;
        let complete = end == manifest.entries.len();
        let digest = self.checkpoint_digest(
            "object namespace manifest page checkpoint",
            &ManifestCheckpointProjection {
                phase,
                binding_digest,
                manifest_digest: &manifest_digest,
                page_index,
                start_index: start as u32,
                next_index: end as u32,
                processed_bytes,
                complete,
                previous_checkpoint_digest: previous
                    .map(|checkpoint| &checkpoint.checkpoint_digest),
                page_entries: &manifest.entries[start..end],
            },
        )?;
        Ok(ObjectNamespaceManifestPageCheckpoint {
            phase: phase.into(),
            binding_digest: binding_digest.clone(),
            manifest_digest,
            page_index,
            start_index: start as u32,
            next_index: end as u32,
            processed_bytes,
            complete,
            checkpoint_digest: digest,
        })
    }

    pub(super) fn validate_complete_manifest_checkpoint(
        &self,
        checkpoint: &ObjectNamespaceManifestPageCheckpoint,
        phase: &str,
        binding_digest: &Sha256Digest,
        manifest: &RecoveryManifest,
    ) -> Result<(), ObjectNamespaceError> {
        let expected_manifest_digest =
            Sha256Digest::from_bytes(&manifest_bytes(manifest, self.maximum_manifest_bytes)?);
        if checkpoint.phase != phase
            || checkpoint.binding_digest != *binding_digest
            || checkpoint.manifest_digest != expected_manifest_digest
            || checkpoint.next_index as usize != manifest.entries.len()
            || checkpoint.processed_bytes != manifest.state_size_bytes
            || !checkpoint.complete
        {
            return Err(corrupt(
                "object namespace manifest checkpoint is incomplete",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_next_seal_page(
        &self,
        page_index: u32,
        previous: Option<&ObjectNamespaceSealPageCheckpoint>,
    ) -> Result<(), ObjectNamespaceError> {
        if page_index >= OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES
            || previous.is_none() && page_index != 0
        {
            return Err(corrupt("object namespace seal page is outside its bound"));
        }
        if let Some(previous) = previous {
            self.validate_seal_checkpoint(previous)?;
            if previous.complete || previous.page_index.checked_add(1) != Some(page_index) {
                return Err(corrupt("object namespace seal page is discontinuous"));
            }
        }
        Ok(())
    }

    pub(super) fn validate_seal_checkpoint(
        &self,
        checkpoint: &ObjectNamespaceSealPageCheckpoint,
    ) -> Result<(), ObjectNamespaceError> {
        if checkpoint.page_index >= OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES
            || checkpoint.entries.is_empty()
            || checkpoint.entries.len() > CHECKPOINT_PAGE_OBJECTS
            || checkpoint
                .entries
                .windows(2)
                .any(|pair| pair[0].key >= pair[1].key)
            || checkpoint
                .start_after
                .as_ref()
                .is_some_and(|cursor| cursor >= &checkpoint.entries[0].key)
            || state_size(&checkpoint.entries)? > self.checkpoint_page_bytes()
            || checkpoint.checkpoint_digest != self.seal_checkpoint_digest(checkpoint)?
        {
            return Err(corrupt("object namespace seal page checkpoint is invalid"));
        }
        Ok(())
    }

    pub(super) fn validate_next_observation_page(
        &self,
        phase: &str,
        binding_digest: &Sha256Digest,
        page_index: u32,
        previous: Option<&ObjectNamespaceObservationPageCheckpoint>,
    ) -> Result<(), ObjectNamespaceError> {
        if page_index >= OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES
            || previous.is_none() && page_index != 0
        {
            return Err(corrupt(
                "object namespace observation page is outside its bound",
            ));
        }
        if let Some(previous) = previous {
            if previous.phase != phase
                || previous.binding_digest != *binding_digest
                || previous.complete
                || previous.page_index.checked_add(1) != Some(page_index)
            {
                return Err(corrupt(
                    "object namespace observation page is discontinuous",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_complete_observation_checkpoint(
        &self,
        checkpoint: &ObjectNamespaceObservationPageCheckpoint,
        phase: &str,
        binding_digest: &Sha256Digest,
    ) -> Result<(), ObjectNamespaceError> {
        if checkpoint.phase != phase
            || checkpoint.binding_digest != *binding_digest
            || !checkpoint.complete
        {
            return Err(corrupt(
                "object namespace observation checkpoint is incomplete",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_next_manifest_page(
        &self,
        phase: &str,
        binding_digest: &Sha256Digest,
        manifest_digest: &Sha256Digest,
        page_index: u32,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<(), ObjectNamespaceError> {
        if page_index >= OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES
            || previous.is_none() && page_index != 0
        {
            return Err(corrupt(
                "object namespace manifest page is outside its bound",
            ));
        }
        if let Some(previous) = previous {
            if previous.phase != phase
                || previous.binding_digest != *binding_digest
                || previous.manifest_digest != *manifest_digest
                || previous.complete
                || previous.page_index.checked_add(1) != Some(page_index)
            {
                return Err(corrupt("object namespace manifest page is discontinuous"));
            }
        }
        Ok(())
    }

    pub(super) fn collect_seal_pages(
        &self,
        pages: &[ObjectNamespaceSealPageCheckpoint],
    ) -> Result<Vec<RecoveryManifestEntry>, ObjectNamespaceError> {
        if pages.is_empty() || pages.len() > OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES as usize {
            return Err(corrupt(
                "object namespace seal checkpoint set is empty or unbounded",
            ));
        }
        let mut entries = Vec::new();
        let mut previous_key: Option<ObjectNamespaceKey> = None;
        for (index, page) in pages.iter().enumerate() {
            self.validate_seal_checkpoint(page)?;
            if page.page_index as usize != index
                || page.start_after != previous_key
                || page.complete != (index + 1 == pages.len())
            {
                return Err(corrupt(
                    "object namespace seal checkpoints are discontinuous",
                ));
            }
            previous_key = page.entries.last().map(|entry| entry.key.clone());
            entries.extend(page.entries.iter().cloned());
        }
        if entries.len() > self.maximum_objects as usize
            || state_size(&entries)? > self.maximum_state_bytes
            || entries.windows(2).any(|pair| pair[0].key >= pair[1].key)
        {
            return Err(corrupt(
                "object namespace seal checkpoints exceed their state bound",
            ));
        }
        Ok(entries)
    }

    pub(super) fn seal_checkpoint_digest(
        &self,
        checkpoint: &ObjectNamespaceSealPageCheckpoint,
    ) -> Result<Sha256Digest, ObjectNamespaceError> {
        self.checkpoint_digest(
            "object namespace seal page checkpoint",
            &SealCheckpointProjection {
                page_index: checkpoint.page_index,
                start_after: &checkpoint.start_after,
                entries: &checkpoint.entries,
                complete: checkpoint.complete,
            },
        )
    }

    pub(super) fn cleanup_checkpoint_digest(
        &self,
        checkpoint: &ObjectNamespaceCleanupPageCheckpoint,
    ) -> Result<Sha256Digest, ObjectNamespaceError> {
        self.checkpoint_digest(
            "object namespace recovery cleanup page checkpoint",
            &CleanupCheckpointProjection {
                binding_digest: &checkpoint.binding_digest,
                page_index: checkpoint.page_index,
                entries: &checkpoint.entries,
                complete: checkpoint.complete,
            },
        )
    }

    pub(super) fn validate_cleanup_checkpoint(
        &self,
        checkpoint: &ObjectNamespaceCleanupPageCheckpoint,
    ) -> Result<(), ObjectNamespaceError> {
        if checkpoint.page_index >= OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES
            || checkpoint.entries.len() > CHECKPOINT_PAGE_OBJECTS
            || checkpoint
                .entries
                .windows(2)
                .any(|pair| pair[0].key >= pair[1].key)
            || checkpoint.checkpoint_digest != self.cleanup_checkpoint_digest(checkpoint)?
        {
            return Err(corrupt(
                "object namespace recovery cleanup checkpoint is invalid",
            ));
        }
        Ok(())
    }

    pub(super) fn anchor_checkpoint_digest(
        &self,
        checkpoint: &ObjectNamespaceRecoveryAnchorCheckpoint,
    ) -> Result<Sha256Digest, ObjectNamespaceError> {
        self.checkpoint_digest(
            "object namespace recovery anchor checkpoint",
            &AnchorCheckpointProjection {
                binding_digest: &checkpoint.binding_digest,
                manifest_digest: &checkpoint.manifest_digest,
            },
        )
    }

    pub(super) fn validate_anchor_checkpoint(
        &self,
        checkpoint: &ObjectNamespaceRecoveryAnchorCheckpoint,
    ) -> Result<(), ObjectNamespaceError> {
        if checkpoint.checkpoint_digest != self.anchor_checkpoint_digest(checkpoint)? {
            return Err(corrupt(
                "object namespace recovery anchor checkpoint is invalid",
            ));
        }
        Ok(())
    }

    fn checkpoint_digest<T: Serialize>(
        &self,
        label: &str,
        value: &T,
    ) -> Result<Sha256Digest, ObjectNamespaceError> {
        let bytes = canonical_json_bounded(value, self.maximum_manifest_bytes, label)
            .map_err(ObjectNamespaceError::Corrupt)?;
        Ok(Sha256Digest::from_bytes(&bytes))
    }

    fn checkpoint_page_bytes(&self) -> u64 {
        self.maximum_state_bytes.min(CHECKPOINT_PAGE_BYTES)
    }

    pub(super) fn page_end_for_listed(
        &self,
        entries: &[ObjectNamespaceEntry],
    ) -> Result<usize, ObjectNamespaceError> {
        bounded_page_end(
            entries.iter().map(|entry| entry.size_bytes),
            entries.len(),
            self.checkpoint_page_bytes(),
        )
    }

    pub(super) fn page_end_for_manifest(
        &self,
        entries: &[RecoveryManifestEntry],
    ) -> Result<usize, ObjectNamespaceError> {
        bounded_page_end(
            entries.iter().map(|entry| entry.size_bytes),
            entries.len(),
            self.checkpoint_page_bytes(),
        )
    }
}

pub(super) fn entries_after<'a>(
    entries: &'a [ObjectNamespaceEntry],
    start_after: Option<&ObjectNamespaceKey>,
) -> &'a [ObjectNamespaceEntry] {
    let start = start_after.map_or(0, |cursor| {
        entries.partition_point(|entry| &entry.key <= cursor)
    });
    &entries[start..]
}

fn bounded_page_end(
    sizes: impl Iterator<Item = u64>,
    length: usize,
    maximum_bytes: u64,
) -> Result<usize, ObjectNamespaceError> {
    if length == 0 {
        return Ok(0);
    }
    let mut total = 0_u64;
    let mut end = 0_usize;
    for size in sizes.take(CHECKPOINT_PAGE_OBJECTS) {
        let next = total
            .checked_add(size)
            .ok_or_else(|| corrupt("object namespace checkpoint page size overflowed"))?;
        if end > 0 && next > maximum_bytes {
            break;
        }
        if next > maximum_bytes {
            return Err(corrupt(
                "object namespace object exceeds the durable checkpoint page bound",
            ));
        }
        total = next;
        end += 1;
    }
    Ok(end)
}

pub(super) fn require_entry_body(
    entry: &RecoveryManifestEntry,
    body: &[u8],
    label: &str,
) -> Result<(), ObjectNamespaceError> {
    if body.len() as u64 != entry.size_bytes || Sha256Digest::from_bytes(body) != entry.digest {
        return Err(corrupt(format!(
            "object namespace {label} does not match its sealed manifest"
        )));
    }
    Ok(())
}

fn require_metadata_subset(
    observed: &[ObjectNamespaceEntry],
    expected: &[RecoveryManifestEntry],
) -> Result<(), ObjectNamespaceError> {
    for entry in observed {
        match expected.binary_search_by(|candidate| candidate.key.cmp(&entry.key)) {
            Ok(index) if expected[index].size_bytes == entry.size_bytes => {}
            _ => {
                return Err(corrupt(
                    "object namespace metadata is outside the exact recovery manifest",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn require_metadata_complete(
    observed: &[ObjectNamespaceEntry],
    expected: &[RecoveryManifestEntry],
) -> Result<(), ObjectNamespaceError> {
    if observed.len() != expected.len()
        || observed.iter().zip(expected).any(|(observed, expected)| {
            observed.key != expected.key || observed.size_bytes != expected.size_bytes
        })
    {
        return Err(corrupt(
            "object namespace metadata does not exactly match the sealed manifest",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_pages_enforce_object_and_byte_bounds() {
        assert_eq!(
            bounded_page_end((0..33).map(|_| 1), 33, CHECKPOINT_PAGE_BYTES)
                .expect("count-bounded page"),
            CHECKPOINT_PAGE_OBJECTS
        );
        assert_eq!(
            bounded_page_end([40, 25].into_iter(), 2, 64).expect("byte-bounded page"),
            1
        );
        assert!(matches!(
            bounded_page_end([65].into_iter(), 1, 64),
            Err(ObjectNamespaceError::Corrupt(_))
        ));
    }
}
