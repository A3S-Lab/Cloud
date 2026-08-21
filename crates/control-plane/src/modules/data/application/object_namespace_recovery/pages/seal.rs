use super::*;

impl ObjectNamespaceRecoveryExecutor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn seal_page(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        previous_point: Option<&ObjectNamespaceRecoveryPoint>,
        writer_epoch: u64,
        writer_fence_receipt_digest: &Sha256Digest,
        sealed_at: DateTime<Utc>,
        page_index: u32,
        previous_page: Option<&ObjectNamespaceSealPageCheckpoint>,
    ) -> Result<ObjectNamespaceSealPageCheckpoint, ObjectNamespaceError> {
        let (sequence, _) = self.validate_seal_request(
            source,
            recovery,
            previous_point,
            writer_epoch,
            writer_fence_receipt_digest,
            sealed_at,
        )?;
        self.validate_next_seal_page(page_index, previous_page)?;

        let listed = self.list_live_entries(source).await?;
        if listed.is_empty() {
            return Err(corrupt(
                "object namespace recovery cannot seal an empty state cut",
            ));
        }
        let start_after =
            previous_page.and_then(|page| page.entries.last().map(|entry| entry.key.clone()));
        let remaining = entries_after(&listed, start_after.as_ref());
        if remaining.is_empty() {
            return Err(corrupt(
                "object namespace seal checkpoint cursor passed the exact state cut",
            ));
        }
        let page_end = self.page_end_for_listed(remaining)?;
        let selected = &remaining[..page_end];
        let mut entries = Vec::with_capacity(selected.len());
        for listed_entry in selected {
            let body = read_exact(
                source.namespace.as_ref(),
                &listed_entry.key,
                self.maximum_object_bytes,
            )
            .await?;
            if body.len() as u64 != listed_entry.size_bytes {
                return Err(corrupt(
                    "object namespace changed between recovery listing and page read",
                ));
            }
            let entry = RecoveryManifestEntry {
                key: listed_entry.key.clone(),
                digest: Sha256Digest::from_bytes(&body),
                size_bytes: listed_entry.size_bytes,
            };
            let recovery_key = snapshot_key(source.namespace_id, sequence, &entry.key)?;
            create_or_verify(
                recovery.namespace.as_ref(),
                &recovery_key,
                body,
                self.maximum_object_bytes,
            )
            .await?;
            entries.push(entry);
        }
        let complete = page_end == remaining.len();
        let mut checkpoint = ObjectNamespaceSealPageCheckpoint {
            page_index,
            start_after,
            entries,
            complete,
            checkpoint_digest: Sha256Digest::from_bytes(b"uninitialized"),
        };
        checkpoint.checkpoint_digest = self.seal_checkpoint_digest(&checkpoint)?;
        Ok(checkpoint)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn verify_seal_page(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        previous_point: Option<&ObjectNamespaceRecoveryPoint>,
        writer_epoch: u64,
        writer_fence_receipt_digest: &Sha256Digest,
        sealed_at: DateTime<Utc>,
        checkpoint: &ObjectNamespaceSealPageCheckpoint,
    ) -> Result<ObjectNamespaceSealPageCheckpoint, ObjectNamespaceError> {
        let (sequence, _) = self.validate_seal_request(
            source,
            recovery,
            previous_point,
            writer_epoch,
            writer_fence_receipt_digest,
            sealed_at,
        )?;
        self.validate_seal_checkpoint(checkpoint)?;
        for entry in &checkpoint.entries {
            let source_body = read_exact(
                source.namespace.as_ref(),
                &entry.key,
                self.maximum_object_bytes,
            )
            .await?;
            let recovery_key = snapshot_key(source.namespace_id, sequence, &entry.key)?;
            let recovery_body = read_exact(
                recovery.namespace.as_ref(),
                &recovery_key,
                self.maximum_object_bytes,
            )
            .await?;
            if source_body != recovery_body
                || source_body.len() as u64 != entry.size_bytes
                || Sha256Digest::from_bytes(&source_body) != entry.digest
            {
                return Err(corrupt(
                    "object namespace seal page changed before its durable verification",
                ));
            }
        }
        Ok(checkpoint.clone())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn finalize_seal_pages(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        previous_point: Option<&ObjectNamespaceRecoveryPoint>,
        writer_epoch: u64,
        writer_fence_receipt_digest: Sha256Digest,
        sealed_at: DateTime<Utc>,
        pages: &[ObjectNamespaceSealPageCheckpoint],
    ) -> Result<ObjectNamespaceRecoveryPoint, ObjectNamespaceError> {
        let (sequence, predecessor_digest) = self.validate_seal_request(
            source,
            recovery,
            previous_point,
            writer_epoch,
            &writer_fence_receipt_digest,
            sealed_at,
        )?;
        let entries = self.collect_seal_pages(pages)?;
        let listed = self.list_live_entries(source).await?;
        require_metadata_complete(&listed, &entries)?;
        let sealed_at = canonical_timestamp(sealed_at);
        let manifest = RecoveryManifest {
            schema: RECOVERY_MANIFEST_SCHEMA.into(),
            namespace_id: source.namespace_id,
            sequence,
            writer_epoch,
            writer_fence_receipt_digest,
            provider_profile_digest: source.provider_profile_digest.clone(),
            predecessor_digest,
            state_digest: state_digest(&entries, self.maximum_manifest_bytes)?,
            state_size_bytes: state_size(&entries)?,
            entries,
            sealed_at,
        };
        self.validate_manifest(&manifest)?;
        let key = manifest_key(source.namespace_id, sequence)?;
        let bytes = manifest_bytes(&manifest, self.maximum_manifest_bytes)?;
        create_or_verify(
            recovery.namespace.as_ref(),
            &key,
            bytes,
            self.maximum_manifest_bytes as u64,
        )
        .await?;
        let point = self.point_from_manifest(manifest, key)?;
        if let Some(previous) = previous_point {
            point
                .validate_successor_of(previous)
                .map_err(ObjectNamespaceError::Corrupt)?;
        }
        Ok(point)
    }
}
