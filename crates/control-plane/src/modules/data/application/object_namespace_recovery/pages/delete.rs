use super::*;

impl ObjectNamespaceRecoveryExecutor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn delete_retained_preflight_page(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        page_index: u32,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<ObjectNamespaceManifestPageCheckpoint, ObjectNamespaceError> {
        self.delete_retained_page(
            source,
            recovery,
            retained,
            point,
            restore_plan,
            restore_evidence,
            deletion_plan,
            retention_policy,
            "delete_retained_preflight",
            page_index,
            previous,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn delete_retained_postflight_page(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        page_index: u32,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<ObjectNamespaceManifestPageCheckpoint, ObjectNamespaceError> {
        self.delete_retained_page(
            source,
            recovery,
            retained,
            point,
            restore_plan,
            restore_evidence,
            deletion_plan,
            retention_policy,
            "delete_retained_postflight",
            page_index,
            previous,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn delete_retained_page(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        phase: &str,
        page_index: u32,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<ObjectNamespaceManifestPageCheckpoint, ObjectNamespaceError> {
        self.validate_delete_request(
            source,
            recovery,
            retained,
            point,
            restore_plan,
            restore_evidence,
            deletion_plan,
            retention_policy,
        )?;
        let manifest = self.load_manifest(recovery, point).await?;
        let listed = self.list_live_entries(retained).await?;
        require_metadata_complete(&listed, &manifest.entries)?;
        let (start, end) = self.manifest_page_range(
            phase,
            deletion_plan.digest(),
            &manifest,
            page_index,
            previous,
        )?;
        for entry in &manifest.entries[start..end] {
            let body = read_exact(
                retained.namespace.as_ref(),
                &entry.key,
                self.maximum_object_bytes,
            )
            .await?;
            require_entry_body(entry, &body, "retained isolated restore")?;
        }
        self.build_manifest_checkpoint(
            phase,
            deletion_plan.digest(),
            &manifest,
            page_index,
            start,
            end,
            previous,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn delete_source_preflight_page(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        page_index: u32,
        previous: Option<&ObjectNamespaceObservationPageCheckpoint>,
    ) -> Result<ObjectNamespaceObservationPageCheckpoint, ObjectNamespaceError> {
        self.validate_delete_request(
            source,
            recovery,
            retained,
            point,
            restore_plan,
            restore_evidence,
            deletion_plan,
            retention_policy,
        )?;
        let manifest = self.load_manifest(recovery, point).await?;
        let marker_key = deletion_marker_key(point, deletion_plan)?;
        let marker_bytes = deletion_marker_bytes(
            point,
            restore_plan,
            deletion_plan,
            self.maximum_manifest_bytes,
        )?;
        let marker_exists = match recovery
            .namespace
            .read(&marker_key, self.maximum_manifest_bytes as u64)
            .await?
        {
            ObjectNamespaceRead::Found { body, .. } if body == marker_bytes => true,
            ObjectNamespaceRead::Found { .. } | ObjectNamespaceRead::Corrupt => {
                return Err(corrupt("object namespace deletion replay marker changed"));
            }
            ObjectNamespaceRead::Missing => false,
        };
        self.observe_manifest_page(
            source,
            &manifest,
            "delete_source_preflight",
            deletion_plan.digest(),
            !marker_exists,
            page_index,
            previous,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn mark_delete(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        retained_checkpoint: &ObjectNamespaceManifestPageCheckpoint,
        source_checkpoint: &ObjectNamespaceObservationPageCheckpoint,
    ) -> Result<(), ObjectNamespaceError> {
        self.validate_delete_request(
            source,
            recovery,
            retained,
            point,
            restore_plan,
            restore_evidence,
            deletion_plan,
            retention_policy,
        )?;
        let manifest = self.load_manifest(recovery, point).await?;
        self.validate_complete_manifest_checkpoint(
            retained_checkpoint,
            "delete_retained_preflight",
            deletion_plan.digest(),
            &manifest,
        )?;
        self.validate_complete_observation_checkpoint(
            source_checkpoint,
            "delete_source_preflight",
            deletion_plan.digest(),
        )?;
        let marker_key = deletion_marker_key(point, deletion_plan)?;
        let marker_bytes = deletion_marker_bytes(
            point,
            restore_plan,
            deletion_plan,
            self.maximum_manifest_bytes,
        )?;
        create_or_verify(
            recovery.namespace.as_ref(),
            &marker_key,
            marker_bytes,
            self.maximum_manifest_bytes as u64,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn delete_source_page(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        page_index: u32,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<ObjectNamespaceManifestPageCheckpoint, ObjectNamespaceError> {
        self.validate_delete_request(
            source,
            recovery,
            retained,
            point,
            restore_plan,
            restore_evidence,
            deletion_plan,
            retention_policy,
        )?;
        let manifest = self.load_manifest(recovery, point).await?;
        let (start, end) = self.manifest_page_range(
            "delete_source",
            deletion_plan.digest(),
            &manifest,
            page_index,
            previous,
        )?;
        for entry in &manifest.entries[start..end] {
            match source
                .namespace
                .read(&entry.key, self.maximum_object_bytes)
                .await?
            {
                ObjectNamespaceRead::Found { body, .. } => {
                    require_entry_body(entry, &body, "source deletion")?;
                    source.namespace.delete(&entry.key).await?;
                }
                ObjectNamespaceRead::Missing => {}
                ObjectNamespaceRead::Corrupt => {
                    return Err(corrupt("object namespace source object is corrupt"));
                }
            }
        }
        self.build_manifest_checkpoint(
            "delete_source",
            deletion_plan.digest(),
            &manifest,
            page_index,
            start,
            end,
            previous,
        )
    }

    pub(crate) async fn confirm_source_absence(
        &self,
        source: &ObjectNamespaceAccess,
        deletion_plan: &ObjectNamespaceDeletionPlan,
    ) -> Result<ObjectNamespaceObservationPageCheckpoint, ObjectNamespaceError> {
        let listed = self.list_live_entries(source).await?;
        if !listed.is_empty() {
            return Err(corrupt("object namespace source remained after deletion"));
        }
        self.build_observation_checkpoint(
            "delete_source_absence",
            deletion_plan.digest(),
            0,
            None,
            &[],
            true,
            None,
        )
    }

    pub(crate) async fn plan_recovery_cleanup_page(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        point: &ObjectNamespaceRecoveryPoint,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        page_index: u32,
        previous: Option<&ObjectNamespaceCleanupPageCheckpoint>,
    ) -> Result<ObjectNamespaceCleanupPageCheckpoint, ObjectNamespaceError> {
        if page_index >= OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES
            || previous.is_some_and(|checkpoint| {
                checkpoint.page_index.checked_add(1) != Some(page_index)
                    || checkpoint.complete
                    || self.validate_cleanup_checkpoint(checkpoint).is_err()
            })
            || previous.is_none() && page_index != 0
        {
            return Err(corrupt(
                "object namespace recovery cleanup page is discontinuous",
            ));
        }
        let prefix = recovery_namespace_prefix(point.spec().namespace_id)?;
        let (maximum_objects, maximum_bytes) = self.recovery_listing_bounds(retention_policy)?;
        let mut listed = recovery
            .namespace
            .list(Some(&prefix), maximum_objects, maximum_bytes)
            .await?;
        listed.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        let anchor = listed
            .iter()
            .find(|entry| entry.key == point.spec().manifest_key)
            .ok_or_else(|| corrupt("object namespace recovery replay anchor is missing"))?;
        if anchor.size_bytes > self.maximum_manifest_bytes as u64 {
            return Err(corrupt(
                "object namespace recovery replay anchor exceeds its bound",
            ));
        }
        let candidates = listed
            .into_iter()
            .filter(|entry| entry.key != point.spec().manifest_key)
            .collect::<Vec<_>>();
        let page_end = self.page_end_for_listed(&candidates)?;
        let mut entries = Vec::with_capacity(page_end);
        for entry in &candidates[..page_end] {
            let body = read_exact(
                recovery.namespace.as_ref(),
                &entry.key,
                self.maximum_object_bytes
                    .max(self.maximum_manifest_bytes as u64),
            )
            .await?;
            if body.len() as u64 != entry.size_bytes {
                return Err(corrupt(
                    "object namespace recovery object changed while freezing its cleanup plan",
                ));
            }
            entries.push(ObjectNamespaceCleanupEntry {
                key: entry.key.clone(),
                size_bytes: entry.size_bytes,
                digest: Sha256Digest::from_bytes(&body),
            });
        }
        let complete = page_end == candidates.len();
        let mut checkpoint = ObjectNamespaceCleanupPageCheckpoint {
            binding_digest: deletion_plan.digest().clone(),
            page_index,
            entries,
            complete,
            checkpoint_digest: Sha256Digest::from_bytes(b"uninitialized"),
        };
        checkpoint.checkpoint_digest = self.cleanup_checkpoint_digest(&checkpoint)?;
        Ok(checkpoint)
    }

    pub(crate) async fn delete_recovery_cleanup_page(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        checkpoint: &ObjectNamespaceCleanupPageCheckpoint,
    ) -> Result<ObjectNamespaceCleanupPageCheckpoint, ObjectNamespaceError> {
        self.validate_cleanup_checkpoint(checkpoint)?;
        if checkpoint.binding_digest != *deletion_plan.digest() {
            return Err(corrupt("object namespace recovery cleanup binding changed"));
        }
        for entry in &checkpoint.entries {
            match recovery
                .namespace
                .read(
                    &entry.key,
                    self.maximum_object_bytes
                        .max(self.maximum_manifest_bytes as u64),
                )
                .await?
            {
                ObjectNamespaceRead::Found { body, .. }
                    if body.len() as u64 == entry.size_bytes
                        && Sha256Digest::from_bytes(&body) == entry.digest =>
                {
                    recovery.namespace.delete(&entry.key).await?;
                }
                ObjectNamespaceRead::Missing => {}
                ObjectNamespaceRead::Found { .. } | ObjectNamespaceRead::Corrupt => {
                    return Err(corrupt(
                        "object namespace recovery cleanup object changed after its plan",
                    ));
                }
            }
        }
        Ok(checkpoint.clone())
    }

    pub(crate) async fn delete_recovery_anchor(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        point: &ObjectNamespaceRecoveryPoint,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
    ) -> Result<ObjectNamespaceRecoveryAnchorCheckpoint, ObjectNamespaceError> {
        let prefix = recovery_namespace_prefix(point.spec().namespace_id)?;
        let (maximum_objects, maximum_bytes) = self.recovery_listing_bounds(retention_policy)?;
        let listed = recovery
            .namespace
            .list(Some(&prefix), maximum_objects, maximum_bytes)
            .await?;
        match listed.as_slice() {
            [] => {}
            [entry] if entry.key == point.spec().manifest_key => {
                let body = read_exact(
                    recovery.namespace.as_ref(),
                    &entry.key,
                    self.maximum_manifest_bytes as u64,
                )
                .await?;
                if Sha256Digest::from_bytes(&body) != point.spec().manifest_digest {
                    return Err(corrupt("object namespace recovery replay anchor changed"));
                }
                recovery.namespace.delete(&entry.key).await?;
            }
            _ => {
                return Err(corrupt(
                    "object namespace recovery objects remain before anchor deletion",
                ));
            }
        }
        let mut checkpoint = ObjectNamespaceRecoveryAnchorCheckpoint {
            binding_digest: deletion_plan.digest().clone(),
            manifest_digest: point.spec().manifest_digest.clone(),
            checkpoint_digest: Sha256Digest::from_bytes(b"uninitialized"),
        };
        checkpoint.checkpoint_digest = self.anchor_checkpoint_digest(&checkpoint)?;
        Ok(checkpoint)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn finalize_delete_pages(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        retained_checkpoint: &ObjectNamespaceManifestPageCheckpoint,
        anchor_checkpoint: &ObjectNamespaceRecoveryAnchorCheckpoint,
        completed_at: DateTime<Utc>,
    ) -> Result<ObjectNamespaceDeletionEvidence, ObjectNamespaceError> {
        self.validate_delete_request(
            source,
            recovery,
            retained,
            point,
            restore_plan,
            restore_evidence,
            deletion_plan,
            retention_policy,
        )?;
        let manifest_digest = anchor_checkpoint.manifest_digest.clone();
        self.validate_anchor_checkpoint(anchor_checkpoint)?;
        if anchor_checkpoint.binding_digest != *deletion_plan.digest()
            || manifest_digest != point.spec().manifest_digest
        {
            return Err(corrupt(
                "object namespace recovery anchor checkpoint changed",
            ));
        }
        if retained_checkpoint.phase != "delete_retained_postflight"
            || retained_checkpoint.binding_digest != *deletion_plan.digest()
            || retained_checkpoint.manifest_digest != point.spec().manifest_digest
            || !retained_checkpoint.complete
        {
            return Err(corrupt(
                "object namespace retained postflight checkpoint is incomplete",
            ));
        }
        let source_entries = self.list_live_entries(source).await?;
        if !source_entries.is_empty() {
            return Err(corrupt("object namespace source remained after deletion"));
        }
        let prefix = recovery_namespace_prefix(point.spec().namespace_id)?;
        let (maximum_objects, maximum_bytes) = self.recovery_listing_bounds(retention_policy)?;
        if !recovery
            .namespace
            .list(Some(&prefix), maximum_objects, maximum_bytes)
            .await?
            .is_empty()
        {
            return Err(corrupt(
                "object namespace recovery storage remained after deletion",
            ));
        }
        let retained_listed = self.list_live_entries(retained).await?;
        if retained_listed.len() != retained_checkpoint.next_index as usize
            || retained_checkpoint.processed_bytes != point.spec().state_size_bytes
        {
            return Err(corrupt(
                "retained isolated restore changed during source cleanup",
            ));
        }
        let state_cleanup_receipt_digest = receipt_digest(
            "object namespace state cleanup receipt",
            &CleanupReceiptProjection {
                deletion_plan_digest: deletion_plan.digest(),
                namespace_id: source.namespace_id,
                kind: "state",
            },
        )?;
        let recovery_cleanup_receipt_digest = receipt_digest(
            "object namespace recovery cleanup receipt",
            &CleanupReceiptProjection {
                deletion_plan_digest: deletion_plan.digest(),
                namespace_id: source.namespace_id,
                kind: "recovery",
            },
        )?;
        let namespace_absence_receipt_digest = receipt_digest(
            "object namespace absence receipt",
            &CleanupReceiptProjection {
                deletion_plan_digest: deletion_plan.digest(),
                namespace_id: source.namespace_id,
                kind: "absence",
            },
        )?;
        let retained_restore_observation_digest = receipt_digest(
            "object namespace retained restore observation",
            &RetainedRestoreProjection {
                deletion_plan_digest: deletion_plan.digest(),
                namespace_id: retained.namespace_id,
                state_digest: &point.spec().state_digest,
                state_size_bytes: point.spec().state_size_bytes,
            },
        )?;
        ObjectNamespaceDeletionEvidence::complete(
            deletion_plan,
            state_cleanup_receipt_digest,
            recovery_cleanup_receipt_digest,
            namespace_absence_receipt_digest,
            retained_restore_observation_digest,
            completed_at,
        )
        .map_err(ObjectNamespaceError::Corrupt)
    }
}
