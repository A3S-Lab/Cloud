use super::*;

impl ObjectNamespaceRecoveryExecutor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn restore_preflight_page(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        target: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        plan: &ObjectNamespaceRestorePlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        page_index: u32,
        previous: Option<&ObjectNamespaceObservationPageCheckpoint>,
    ) -> Result<ObjectNamespaceObservationPageCheckpoint, ObjectNamespaceError> {
        plan.validate_source(point, retention_policy)
            .map_err(ObjectNamespaceError::Invalid)?;
        self.validate_restore_bindings(recovery, target, point, plan)?;
        let manifest = self.load_manifest(recovery, point).await?;
        self.observe_manifest_page(
            target,
            &manifest,
            "restore_preflight",
            point.digest(),
            false,
            page_index,
            previous,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn restore_apply_page(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        target: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        plan: &ObjectNamespaceRestorePlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        page_index: u32,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<ObjectNamespaceManifestPageCheckpoint, ObjectNamespaceError> {
        plan.validate_source(point, retention_policy)
            .map_err(ObjectNamespaceError::Invalid)?;
        self.validate_restore_bindings(recovery, target, point, plan)?;
        let manifest = self.load_manifest(recovery, point).await?;
        let (start, end) = self.manifest_page_range(
            "restore_apply",
            point.digest(),
            &manifest,
            page_index,
            previous,
        )?;
        for entry in &manifest.entries[start..end] {
            let key = snapshot_key(point.spec().namespace_id, point.spec().sequence, &entry.key)?;
            let body =
                read_exact(recovery.namespace.as_ref(), &key, self.maximum_object_bytes).await?;
            require_entry_body(entry, &body, "recovery snapshot")?;
            create_or_verify(
                target.namespace.as_ref(),
                &entry.key,
                body,
                self.maximum_object_bytes,
            )
            .await?;
        }
        self.build_manifest_checkpoint(
            "restore_apply",
            point.digest(),
            &manifest,
            page_index,
            start,
            end,
            previous,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn restore_verify_page(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        target: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        plan: &ObjectNamespaceRestorePlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        page_index: u32,
        previous: Option<&ObjectNamespaceManifestPageCheckpoint>,
    ) -> Result<ObjectNamespaceManifestPageCheckpoint, ObjectNamespaceError> {
        plan.validate_source(point, retention_policy)
            .map_err(ObjectNamespaceError::Invalid)?;
        self.validate_restore_bindings(recovery, target, point, plan)?;
        let manifest = self.load_manifest(recovery, point).await?;
        let listed = self.list_live_entries(target).await?;
        require_metadata_complete(&listed, &manifest.entries)?;
        let (start, end) = self.manifest_page_range(
            "restore_verify",
            point.digest(),
            &manifest,
            page_index,
            previous,
        )?;
        for entry in &manifest.entries[start..end] {
            let body = read_exact(
                target.namespace.as_ref(),
                &entry.key,
                self.maximum_object_bytes,
            )
            .await?;
            require_entry_body(entry, &body, "isolated restore")?;
        }
        self.build_manifest_checkpoint(
            "restore_verify",
            point.digest(),
            &manifest,
            page_index,
            start,
            end,
            previous,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn finalize_restore_pages(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        target: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        plan: &ObjectNamespaceRestorePlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        preflight: &ObjectNamespaceObservationPageCheckpoint,
        verification: &ObjectNamespaceManifestPageCheckpoint,
        verified_at: DateTime<Utc>,
    ) -> Result<ObjectNamespaceRestoreEvidence, ObjectNamespaceError> {
        plan.validate_source(point, retention_policy)
            .map_err(ObjectNamespaceError::Invalid)?;
        self.validate_restore_bindings(recovery, target, point, plan)?;
        let manifest = self.load_manifest(recovery, point).await?;
        self.validate_complete_observation_checkpoint(
            preflight,
            "restore_preflight",
            point.digest(),
        )?;
        self.validate_complete_manifest_checkpoint(
            verification,
            "restore_verify",
            point.digest(),
            &manifest,
        )?;
        let source_manifest_digest = self
            .read_manifest_digest(recovery, &point.spec().manifest_key)
            .await?;
        if source_manifest_digest != point.spec().manifest_digest {
            return Err(corrupt(
                "object namespace recovery source changed during isolated restore",
            ));
        }
        let provider_receipt_digest = receipt_digest(
            "object namespace restore provider receipt",
            &RestoreReceiptProjection {
                plan_digest: plan.digest(),
                source_manifest_digest: &source_manifest_digest,
                target_namespace_id: target.namespace_id,
                restored_state_digest: &manifest.state_digest,
                restored_state_size_bytes: manifest.state_size_bytes,
            },
        )?;
        ObjectNamespaceRestoreEvidence::verified(plan, provider_receipt_digest, verified_at)
            .map_err(ObjectNamespaceError::Corrupt)
    }
}
