use crate::modules::data::domain::{
    IObjectNamespace, ObjectNamespaceDeletionEvidence, ObjectNamespaceDeletionPlan,
    ObjectNamespaceError, ObjectNamespaceKey, ObjectNamespaceRead, ObjectNamespaceRecoveryPoint,
    ObjectNamespaceRecoveryPointSpec, ObjectNamespaceRestoreEvidence, ObjectNamespaceRestorePlan,
    ObjectNamespaceRetentionPolicy,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, Sha256Digest, StorageNamespaceId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const RECOVERY_MANIFEST_SCHEMA: &str = "a3s.s0.object-namespace.recovery-manifest.v1";

/// One exact logical namespace bound to the sole S0 object port.
///
/// The identity and provider digest are kept beside the already-scoped port so
/// restore/delete code cannot accidentally substitute another namespace while
/// reusing otherwise valid recovery contracts.
#[derive(Clone)]
pub struct ObjectNamespaceAccess {
    namespace_id: StorageNamespaceId,
    provider_profile_digest: Sha256Digest,
    namespace: Arc<dyn IObjectNamespace>,
}

impl ObjectNamespaceAccess {
    pub fn new(
        namespace_id: StorageNamespaceId,
        provider_profile_digest: Sha256Digest,
        namespace: Arc<dyn IObjectNamespace>,
    ) -> Result<Self, String> {
        if namespace_id.as_uuid().is_nil()
            || Sha256Digest::parse(provider_profile_digest.as_str())? != provider_profile_digest
        {
            return Err("object namespace access identity is invalid".into());
        }
        Ok(Self {
            namespace_id,
            provider_profile_digest,
            namespace,
        })
    }

    pub const fn namespace_id(&self) -> StorageNamespaceId {
        self.namespace_id
    }

    pub const fn provider_profile_digest(&self) -> &Sha256Digest {
        &self.provider_profile_digest
    }
}

/// Provider-owned immutable recovery storage reached through the same S0 port.
/// It is deliberately not a repository, scheduler, worker, or second client.
#[derive(Clone)]
pub struct ObjectNamespaceRecoveryStore {
    provider_profile_digest: Sha256Digest,
    namespace: Arc<dyn IObjectNamespace>,
}

impl ObjectNamespaceRecoveryStore {
    pub fn new(
        provider_profile_digest: Sha256Digest,
        namespace: Arc<dyn IObjectNamespace>,
    ) -> Result<Self, String> {
        if Sha256Digest::parse(provider_profile_digest.as_str())? != provider_profile_digest {
            return Err("object namespace recovery-store provider identity is invalid".into());
        }
        Ok(Self {
            provider_profile_digest,
            namespace,
        })
    }

    pub const fn provider_profile_digest(&self) -> &Sha256Digest {
        &self.provider_profile_digest
    }
}

/// Bounded S0 execution for a writer-fenced seal, isolated restore, and
/// grace-delayed source deletion.
///
/// Long-running ownership, retries, authorization, and scheduling remain with
/// Operations/Flow. This executor is deterministic and replay-safe so that an
/// owning workflow may call it again after interruption without inventing a
/// second lifecycle or evidence store.
#[derive(Debug, Clone)]
pub struct ObjectNamespaceRecoveryExecutor {
    maximum_objects: u32,
    maximum_object_bytes: u64,
    maximum_state_bytes: u64,
    maximum_manifest_bytes: usize,
}

impl ObjectNamespaceRecoveryExecutor {
    pub fn new(
        maximum_objects: u32,
        maximum_object_bytes: u64,
        maximum_state_bytes: u64,
        maximum_manifest_bytes: usize,
    ) -> Result<Self, String> {
        if maximum_objects == 0
            || maximum_object_bytes == 0
            || maximum_state_bytes < maximum_object_bytes
            || maximum_manifest_bytes == 0
            || maximum_manifest_bytes as u64 > maximum_state_bytes
        {
            return Err("object namespace recovery bounds are invalid".into());
        }
        Ok(Self {
            maximum_objects,
            maximum_object_bytes,
            maximum_state_bytes,
            maximum_manifest_bytes,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn seal(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        previous: Option<&ObjectNamespaceRecoveryPoint>,
        writer_epoch: u64,
        writer_fence_receipt_digest: Sha256Digest,
        sealed_at: DateTime<Utc>,
    ) -> Result<ObjectNamespaceRecoveryPoint, ObjectNamespaceError> {
        self.validate_source_recovery_binding(source, recovery)?;
        if writer_epoch == 0 {
            return Err(invalid("object namespace writer epoch must be positive"));
        }
        canonical_digest(&writer_fence_receipt_digest).map_err(ObjectNamespaceError::Invalid)?;
        let (sequence, predecessor_digest) = match previous {
            Some(point) => {
                point.validate().map_err(ObjectNamespaceError::Invalid)?;
                if point.spec().namespace_id != source.namespace_id
                    || point.spec().provider_profile_digest != source.provider_profile_digest
                {
                    return Err(invalid(
                        "object namespace recovery predecessor has another exact source",
                    ));
                }
                (
                    point.spec().sequence.checked_add(1).ok_or_else(|| {
                        invalid("object namespace recovery sequence is exhausted")
                    })?,
                    Some(point.digest().clone()),
                )
            }
            None => (1, None),
        };
        let sealed_at = canonical_timestamp(sealed_at);
        if previous.is_some_and(|point| {
            writer_epoch < point.spec().writer_epoch || sealed_at < point.spec().sealed_at
        }) {
            return Err(invalid(
                "object namespace recovery cannot regress its writer epoch or seal time",
            ));
        }
        let manifest_key = manifest_key(source.namespace_id, sequence)?;

        if let Some(manifest) = self
            .read_optional_manifest(&recovery.namespace, &manifest_key)
            .await?
        {
            self.validate_manifest_request(
                &manifest,
                SealManifestRequest {
                    source,
                    sequence,
                    writer_epoch,
                    writer_fence_receipt_digest: &writer_fence_receipt_digest,
                    predecessor_digest: predecessor_digest.as_ref(),
                    sealed_at,
                },
            )?;
            self.verify_snapshots(&recovery.namespace, &manifest)
                .await?;
            let point = self.point_from_manifest(manifest, manifest_key)?;
            if let Some(previous) = previous {
                point
                    .validate_successor_of(previous)
                    .map_err(ObjectNamespaceError::Corrupt)?;
            }
            return Ok(point);
        }

        let listed = source
            .namespace
            .list(None, self.maximum_objects, self.maximum_state_bytes)
            .await?;
        if listed.is_empty() {
            return Err(corrupt(
                "object namespace recovery cannot seal an empty state cut",
            ));
        }
        let mut entries = Vec::with_capacity(listed.len());
        for listed_entry in listed {
            if listed_entry.size_bytes > self.maximum_object_bytes {
                return Err(corrupt(
                    "object namespace recovery object exceeds its admission bound",
                ));
            }
            let body = read_exact(
                source.namespace.as_ref(),
                &listed_entry.key,
                self.maximum_object_bytes,
            )
            .await?;
            if body.len() as u64 != listed_entry.size_bytes {
                return Err(corrupt(
                    "object namespace changed between recovery listing and read",
                ));
            }
            let entry = RecoveryManifestEntry {
                key: listed_entry.key,
                digest: Sha256Digest::from_bytes(&body),
                size_bytes: body.len() as u64,
            };
            let snapshot_key = snapshot_key(source.namespace_id, sequence, &entry.key)?;
            create_or_verify(
                recovery.namespace.as_ref(),
                &snapshot_key,
                body,
                self.maximum_object_bytes,
            )
            .await?;
            entries.push(entry);
        }
        entries.sort_unstable_by(|left, right| left.key.as_str().cmp(right.key.as_str()));
        let observed = self.observe(source).await?;
        if observed.entries != entries {
            return Err(corrupt(
                "object namespace changed while its recovery point was sealed",
            ));
        }
        let state_digest = state_digest(&entries, self.maximum_manifest_bytes)?;
        let state_size_bytes = state_size(&entries)?;
        let manifest = RecoveryManifest {
            schema: RECOVERY_MANIFEST_SCHEMA.into(),
            namespace_id: source.namespace_id,
            sequence,
            writer_epoch,
            writer_fence_receipt_digest,
            provider_profile_digest: source.provider_profile_digest.clone(),
            predecessor_digest,
            entries,
            state_digest,
            state_size_bytes,
            sealed_at,
        };
        self.validate_manifest(&manifest)?;
        let manifest_bytes = manifest_bytes(&manifest, self.maximum_manifest_bytes)?;
        create_or_verify(
            recovery.namespace.as_ref(),
            &manifest_key,
            manifest_bytes,
            self.maximum_manifest_bytes as u64,
        )
        .await?;
        let point = self.point_from_manifest(manifest, manifest_key)?;
        if let Some(previous) = previous {
            point
                .validate_successor_of(previous)
                .map_err(ObjectNamespaceError::Corrupt)?;
        }
        Ok(point)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn restore(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        target: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        plan: &ObjectNamespaceRestorePlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        verified_at: DateTime<Utc>,
    ) -> Result<ObjectNamespaceRestoreEvidence, ObjectNamespaceError> {
        plan.validate_source(point, retention_policy)
            .map_err(ObjectNamespaceError::Invalid)?;
        self.validate_restore_bindings(recovery, target, point, plan)?;
        let manifest = self.load_manifest(recovery, point).await?;
        let initial_target = self.observe(target).await?;
        require_subset(&initial_target.entries, &manifest.entries)?;

        for entry in &manifest.entries {
            let snapshot_key =
                snapshot_key(point.spec().namespace_id, point.spec().sequence, &entry.key)?;
            let body = read_exact(
                recovery.namespace.as_ref(),
                &snapshot_key,
                self.maximum_object_bytes,
            )
            .await?;
            if body.len() as u64 != entry.size_bytes
                || Sha256Digest::from_bytes(&body) != entry.digest
            {
                return Err(corrupt(
                    "object namespace recovery snapshot does not match its sealed manifest",
                ));
            }
            create_or_verify(
                target.namespace.as_ref(),
                &entry.key,
                body,
                self.maximum_object_bytes,
            )
            .await?;
        }

        let restored = self.observe(target).await?;
        require_complete(&restored, &manifest)?;
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
                restored_state_digest: &restored.digest,
                restored_state_size_bytes: restored.size_bytes,
            },
        )?;
        ObjectNamespaceRestoreEvidence::verified(plan, provider_receipt_digest, verified_at)
            .map_err(ObjectNamespaceError::Corrupt)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn delete(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained_restore: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        restore_evidence: &ObjectNamespaceRestoreEvidence,
        deletion_plan: &ObjectNamespaceDeletionPlan,
        retention_policy: &ObjectNamespaceRetentionPolicy,
        completed_at: DateTime<Utc>,
    ) -> Result<ObjectNamespaceDeletionEvidence, ObjectNamespaceError> {
        deletion_plan
            .validate_against(point, restore_plan, restore_evidence, retention_policy)
            .map_err(ObjectNamespaceError::Invalid)?;
        self.validate_deletion_bindings(
            source,
            recovery,
            retained_restore,
            point,
            restore_plan,
            deletion_plan,
        )?;
        let completed_at = canonical_timestamp(completed_at);
        if completed_at < deletion_plan.spec().not_before {
            return Err(ObjectNamespaceError::Precondition(
                "object namespace deletion grace period has not elapsed".into(),
            ));
        }

        let retained = self.observe(retained_restore).await?;
        if retained.digest != point.spec().state_digest
            || retained.size_bytes != point.spec().state_size_bytes
        {
            return Err(corrupt(
                "retained isolated restore no longer matches the deletion plan",
            ));
        }

        let manifest = self
            .read_optional_manifest(&recovery.namespace, &point.spec().manifest_key)
            .await?;
        if let Some(manifest) = &manifest {
            self.validate_manifest_for_point(manifest, point)?;
            require_complete(&retained, manifest)?;
        }

        let remaining_source = self.observe(source).await?;
        match &manifest {
            Some(manifest) => {
                let marker_key = deletion_marker_key(point, deletion_plan)?;
                let marker_bytes = deletion_marker_bytes(
                    point,
                    restore_plan,
                    deletion_plan,
                    self.maximum_manifest_bytes,
                )?;
                match recovery
                    .namespace
                    .read(&marker_key, self.maximum_manifest_bytes as u64)
                    .await?
                {
                    ObjectNamespaceRead::Found { body, .. } if body == marker_bytes => {
                        require_subset(&remaining_source.entries, &manifest.entries)?;
                    }
                    ObjectNamespaceRead::Found { .. } | ObjectNamespaceRead::Corrupt => {
                        return Err(corrupt("object namespace deletion replay marker changed"));
                    }
                    ObjectNamespaceRead::Missing => {
                        require_complete(&remaining_source, manifest)?;
                        create_or_verify(
                            recovery.namespace.as_ref(),
                            &marker_key,
                            marker_bytes,
                            self.maximum_manifest_bytes as u64,
                        )
                        .await?;
                    }
                }
            }
            None if !remaining_source.entries.is_empty() => {
                return Err(corrupt(
                    "source state remains after its recovery manifest disappeared",
                ))
            }
            None => {}
        }
        for entry in &remaining_source.entries {
            source.namespace.delete(&entry.key).await?;
        }
        let absent_source = self.observe(source).await?;
        if !absent_source.entries.is_empty() {
            return Err(corrupt("object namespace source remained after deletion"));
        }

        let recovery_prefix = recovery_namespace_prefix(source.namespace_id)?;
        let (maximum_recovery_objects, maximum_recovery_bytes) =
            self.recovery_listing_bounds(retention_policy)?;
        let mut recovery_entries = recovery
            .namespace
            .list(
                Some(&recovery_prefix),
                maximum_recovery_objects,
                maximum_recovery_bytes,
            )
            .await?;
        if manifest.is_none() && !recovery_entries.is_empty() {
            return Err(corrupt(
                "recovery objects remain after the latest manifest disappeared",
            ));
        }
        // The latest manifest is the replay anchor. Delete every snapshot and
        // older point first, then remove that anchor last.
        recovery_entries.sort_unstable_by(|left, right| {
            let left_is_anchor = left.key == point.spec().manifest_key;
            let right_is_anchor = right.key == point.spec().manifest_key;
            left_is_anchor
                .cmp(&right_is_anchor)
                .then_with(|| left.key.as_str().cmp(right.key.as_str()))
        });
        for entry in &recovery_entries {
            recovery.namespace.delete(&entry.key).await?;
        }
        if !recovery
            .namespace
            .list(
                Some(&recovery_prefix),
                maximum_recovery_objects,
                maximum_recovery_bytes,
            )
            .await?
            .is_empty()
        {
            return Err(corrupt(
                "object namespace recovery storage remained after deletion",
            ));
        }

        let retained_after_cleanup = self.observe(retained_restore).await?;
        if retained_after_cleanup != retained {
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
                namespace_id: retained_restore.namespace_id,
                state_digest: &retained_after_cleanup.digest,
                state_size_bytes: retained_after_cleanup.size_bytes,
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

    async fn observe(
        &self,
        access: &ObjectNamespaceAccess,
    ) -> Result<StateObservation, ObjectNamespaceError> {
        let listed = access
            .namespace
            .list(None, self.maximum_objects, self.maximum_state_bytes)
            .await?;
        let mut entries = Vec::with_capacity(listed.len());
        for listed_entry in listed {
            if listed_entry.size_bytes > self.maximum_object_bytes {
                return Err(corrupt(
                    "object namespace observation exceeded its per-object bound",
                ));
            }
            let body = read_exact(
                access.namespace.as_ref(),
                &listed_entry.key,
                self.maximum_object_bytes,
            )
            .await?;
            if body.len() as u64 != listed_entry.size_bytes {
                return Err(corrupt(
                    "object namespace changed between observation listing and read",
                ));
            }
            entries.push(RecoveryManifestEntry {
                key: listed_entry.key,
                digest: Sha256Digest::from_bytes(&body),
                size_bytes: body.len() as u64,
            });
        }
        entries.sort_unstable_by(|left, right| left.key.as_str().cmp(right.key.as_str()));
        let size_bytes = state_size(&entries)?;
        Ok(StateObservation {
            digest: state_digest(&entries, self.maximum_manifest_bytes)?,
            entries,
            size_bytes,
        })
    }

    async fn load_manifest(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        point: &ObjectNamespaceRecoveryPoint,
    ) -> Result<RecoveryManifest, ObjectNamespaceError> {
        let manifest = self
            .read_optional_manifest(&recovery.namespace, &point.spec().manifest_key)
            .await?
            .ok_or_else(|| corrupt("object namespace recovery manifest is missing"))?;
        self.validate_manifest_for_point(&manifest, point)?;
        Ok(manifest)
    }

    async fn read_optional_manifest(
        &self,
        namespace: &Arc<dyn IObjectNamespace>,
        key: &ObjectNamespaceKey,
    ) -> Result<Option<RecoveryManifest>, ObjectNamespaceError> {
        match namespace
            .read(key, self.maximum_manifest_bytes as u64)
            .await?
        {
            ObjectNamespaceRead::Found { body, .. } => {
                let manifest: RecoveryManifest = serde_json::from_slice(&body).map_err(|_| {
                    corrupt("object namespace recovery manifest is not canonical JSON")
                })?;
                if manifest_bytes(&manifest, self.maximum_manifest_bytes)? != body {
                    return Err(corrupt(
                        "object namespace recovery manifest is not canonical",
                    ));
                }
                self.validate_manifest(&manifest)?;
                Ok(Some(manifest))
            }
            ObjectNamespaceRead::Missing => Ok(None),
            ObjectNamespaceRead::Corrupt => {
                Err(corrupt("object namespace recovery manifest is corrupt"))
            }
        }
    }

    async fn read_manifest_digest(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        key: &ObjectNamespaceKey,
    ) -> Result<Sha256Digest, ObjectNamespaceError> {
        match recovery
            .namespace
            .read(key, self.maximum_manifest_bytes as u64)
            .await?
        {
            ObjectNamespaceRead::Found { body, .. } => Ok(Sha256Digest::from_bytes(&body)),
            ObjectNamespaceRead::Missing => {
                Err(corrupt("object namespace recovery manifest is missing"))
            }
            ObjectNamespaceRead::Corrupt => {
                Err(corrupt("object namespace recovery manifest is corrupt"))
            }
        }
    }

    fn point_from_manifest(
        &self,
        manifest: RecoveryManifest,
        manifest_key: ObjectNamespaceKey,
    ) -> Result<ObjectNamespaceRecoveryPoint, ObjectNamespaceError> {
        self.validate_manifest(&manifest)?;
        let manifest_digest =
            Sha256Digest::from_bytes(&manifest_bytes(&manifest, self.maximum_manifest_bytes)?);
        ObjectNamespaceRecoveryPoint::seal(ObjectNamespaceRecoveryPointSpec {
            namespace_id: manifest.namespace_id,
            sequence: manifest.sequence,
            writer_epoch: manifest.writer_epoch,
            provider_profile_digest: manifest.provider_profile_digest,
            manifest_key,
            manifest_digest,
            state_digest: manifest.state_digest,
            state_size_bytes: manifest.state_size_bytes,
            predecessor_digest: manifest.predecessor_digest,
            sealed_at: manifest.sealed_at,
        })
        .map_err(ObjectNamespaceError::Corrupt)
    }

    fn validate_manifest(&self, manifest: &RecoveryManifest) -> Result<(), ObjectNamespaceError> {
        if manifest.schema != RECOVERY_MANIFEST_SCHEMA
            || manifest.namespace_id.as_uuid().is_nil()
            || manifest.sequence == 0
            || manifest.writer_epoch == 0
            || manifest.entries.is_empty()
            || manifest.entries.len() > self.maximum_objects as usize
            || manifest.sealed_at != canonical_timestamp(manifest.sealed_at)
        {
            return Err(corrupt("object namespace recovery manifest is invalid"));
        }
        for digest in [
            &manifest.writer_fence_receipt_digest,
            &manifest.provider_profile_digest,
            &manifest.state_digest,
        ] {
            canonical_digest(digest).map_err(ObjectNamespaceError::Corrupt)?;
        }
        let predecessor_shape_is_valid = match (&manifest.predecessor_digest, manifest.sequence) {
            (None, 1) => true,
            (Some(digest), sequence) if sequence > 1 => canonical_digest(digest).is_ok(),
            _ => false,
        };
        for entry in &manifest.entries {
            ObjectNamespaceKey::parse(entry.key.as_str().to_owned())
                .map_err(ObjectNamespaceError::Corrupt)?;
            canonical_digest(&entry.digest).map_err(ObjectNamespaceError::Corrupt)?;
            if entry.size_bytes > self.maximum_object_bytes {
                return Err(corrupt(
                    "object namespace recovery manifest exceeds its object bound",
                ));
            }
        }
        if !predecessor_shape_is_valid
            || manifest
                .entries
                .windows(2)
                .any(|pair| pair[0].key.as_str() >= pair[1].key.as_str())
            || state_size(&manifest.entries)? != manifest.state_size_bytes
            || manifest.state_size_bytes == 0
            || manifest.state_size_bytes > self.maximum_state_bytes
            || state_digest(&manifest.entries, self.maximum_manifest_bytes)?
                != manifest.state_digest
        {
            return Err(corrupt(
                "object namespace recovery manifest state identity is invalid",
            ));
        }
        Ok(())
    }

    fn validate_manifest_request(
        &self,
        manifest: &RecoveryManifest,
        request: SealManifestRequest<'_>,
    ) -> Result<(), ObjectNamespaceError> {
        self.validate_manifest(manifest)?;
        if manifest.namespace_id != request.source.namespace_id
            || manifest.provider_profile_digest != request.source.provider_profile_digest
            || manifest.sequence != request.sequence
            || manifest.writer_epoch != request.writer_epoch
            || &manifest.writer_fence_receipt_digest != request.writer_fence_receipt_digest
            || manifest.predecessor_digest.as_ref() != request.predecessor_digest
            || manifest.sealed_at != request.sealed_at
        {
            return Err(corrupt(
                "object namespace recovery replay changed its exact request",
            ));
        }
        Ok(())
    }

    fn validate_manifest_for_point(
        &self,
        manifest: &RecoveryManifest,
        point: &ObjectNamespaceRecoveryPoint,
    ) -> Result<(), ObjectNamespaceError> {
        self.validate_manifest(manifest)?;
        point.validate().map_err(ObjectNamespaceError::Corrupt)?;
        let manifest_digest =
            Sha256Digest::from_bytes(&manifest_bytes(manifest, self.maximum_manifest_bytes)?);
        if manifest.namespace_id != point.spec().namespace_id
            || manifest.sequence != point.spec().sequence
            || manifest.writer_epoch != point.spec().writer_epoch
            || manifest.provider_profile_digest != point.spec().provider_profile_digest
            || manifest.predecessor_digest != point.spec().predecessor_digest
            || manifest_digest != point.spec().manifest_digest
            || manifest.state_digest != point.spec().state_digest
            || manifest.state_size_bytes != point.spec().state_size_bytes
            || manifest.sealed_at != point.spec().sealed_at
        {
            return Err(corrupt(
                "object namespace recovery manifest does not match its sealed point",
            ));
        }
        Ok(())
    }

    fn validate_source_recovery_binding(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
    ) -> Result<(), ObjectNamespaceError> {
        if source.provider_profile_digest != recovery.provider_profile_digest {
            return Err(invalid(
                "object namespace source and recovery store use different provider profiles",
            ));
        }
        Ok(())
    }

    fn validate_restore_bindings(
        &self,
        recovery: &ObjectNamespaceRecoveryStore,
        target: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        plan: &ObjectNamespaceRestorePlan,
    ) -> Result<(), ObjectNamespaceError> {
        if recovery.provider_profile_digest != point.spec().provider_profile_digest
            || target.namespace_id != plan.spec().target_namespace_id
            || target.provider_profile_digest != plan.spec().target_provider_profile_digest
        {
            return Err(invalid(
                "object namespace restore substituted a source or target binding",
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_deletion_bindings(
        &self,
        source: &ObjectNamespaceAccess,
        recovery: &ObjectNamespaceRecoveryStore,
        retained_restore: &ObjectNamespaceAccess,
        point: &ObjectNamespaceRecoveryPoint,
        restore_plan: &ObjectNamespaceRestorePlan,
        deletion_plan: &ObjectNamespaceDeletionPlan,
    ) -> Result<(), ObjectNamespaceError> {
        if source.namespace_id != point.spec().namespace_id
            || source.provider_profile_digest != point.spec().provider_profile_digest
            || recovery.provider_profile_digest != point.spec().provider_profile_digest
            || retained_restore.namespace_id != restore_plan.spec().target_namespace_id
            || retained_restore.namespace_id != deletion_plan.spec().retained_restore_namespace_id
            || retained_restore.provider_profile_digest
                != restore_plan.spec().target_provider_profile_digest
        {
            return Err(invalid(
                "object namespace deletion substituted an exact source or retained restore",
            ));
        }
        Ok(())
    }

    fn recovery_listing_bounds(
        &self,
        policy: &ObjectNamespaceRetentionPolicy,
    ) -> Result<(u32, u64), ObjectNamespaceError> {
        let recovery_points = policy.spec().maximum_sealed_recovery_points;
        let objects_per_point = self
            .maximum_objects
            .checked_add(2)
            .ok_or_else(|| invalid("object namespace recovery object bound overflowed"))?;
        let maximum_objects = objects_per_point
            .checked_mul(recovery_points)
            .ok_or_else(|| invalid("object namespace recovery listing bound overflowed"))?;
        let bytes_per_point = self
            .maximum_state_bytes
            .checked_add(self.maximum_manifest_bytes as u64)
            .and_then(|bound| bound.checked_add(self.maximum_manifest_bytes as u64))
            .ok_or_else(|| invalid("object namespace recovery byte bound overflowed"))?;
        let maximum_bytes = bytes_per_point
            .checked_mul(recovery_points as u64)
            .ok_or_else(|| invalid("object namespace recovery listing bound overflowed"))?;
        Ok((maximum_objects, maximum_bytes))
    }

    async fn verify_snapshots(
        &self,
        recovery: &Arc<dyn IObjectNamespace>,
        manifest: &RecoveryManifest,
    ) -> Result<(), ObjectNamespaceError> {
        for entry in &manifest.entries {
            let key = snapshot_key(manifest.namespace_id, manifest.sequence, &entry.key)?;
            let body = read_exact(recovery.as_ref(), &key, self.maximum_object_bytes).await?;
            if body.len() as u64 != entry.size_bytes
                || Sha256Digest::from_bytes(&body) != entry.digest
            {
                return Err(corrupt(
                    "object namespace recovery snapshot changed after sealing",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryManifestEntry {
    key: ObjectNamespaceKey,
    digest: Sha256Digest,
    size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryManifest {
    schema: String,
    namespace_id: StorageNamespaceId,
    sequence: u64,
    writer_epoch: u64,
    writer_fence_receipt_digest: Sha256Digest,
    provider_profile_digest: Sha256Digest,
    predecessor_digest: Option<Sha256Digest>,
    entries: Vec<RecoveryManifestEntry>,
    state_digest: Sha256Digest,
    state_size_bytes: u64,
    sealed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StateObservation {
    entries: Vec<RecoveryManifestEntry>,
    digest: Sha256Digest,
    size_bytes: u64,
}

struct SealManifestRequest<'a> {
    source: &'a ObjectNamespaceAccess,
    sequence: u64,
    writer_epoch: u64,
    writer_fence_receipt_digest: &'a Sha256Digest,
    predecessor_digest: Option<&'a Sha256Digest>,
    sealed_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct StateDigestProjection<'a> {
    entries: &'a [RecoveryManifestEntry],
}

#[derive(Serialize)]
struct RestoreReceiptProjection<'a> {
    plan_digest: &'a Sha256Digest,
    source_manifest_digest: &'a Sha256Digest,
    target_namespace_id: StorageNamespaceId,
    restored_state_digest: &'a Sha256Digest,
    restored_state_size_bytes: u64,
}

#[derive(Serialize)]
struct CleanupReceiptProjection<'a> {
    deletion_plan_digest: &'a Sha256Digest,
    namespace_id: StorageNamespaceId,
    kind: &'static str,
}

#[derive(Serialize)]
struct RetainedRestoreProjection<'a> {
    deletion_plan_digest: &'a Sha256Digest,
    namespace_id: StorageNamespaceId,
    state_digest: &'a Sha256Digest,
    state_size_bytes: u64,
}

#[derive(Serialize)]
struct DeletionMarkerProjection<'a> {
    schema: &'static str,
    deletion_plan_digest: &'a Sha256Digest,
    recovery_point_digest: &'a Sha256Digest,
    restore_plan_digest: &'a Sha256Digest,
    source_namespace_id: StorageNamespaceId,
    retained_restore_namespace_id: StorageNamespaceId,
}

async fn read_exact(
    namespace: &dyn IObjectNamespace,
    key: &ObjectNamespaceKey,
    maximum_bytes: u64,
) -> Result<Vec<u8>, ObjectNamespaceError> {
    match namespace.read(key, maximum_bytes).await? {
        ObjectNamespaceRead::Found { body, .. } => Ok(body),
        ObjectNamespaceRead::Missing => Err(corrupt("object namespace object is missing")),
        ObjectNamespaceRead::Corrupt => Err(corrupt("object namespace object is corrupt")),
    }
}

async fn create_or_verify(
    namespace: &dyn IObjectNamespace,
    key: &ObjectNamespaceKey,
    body: Vec<u8>,
    maximum_bytes: u64,
) -> Result<(), ObjectNamespaceError> {
    match namespace
        .conditional_create(key, body.clone(), maximum_bytes)
        .await
    {
        Ok(_) => Ok(()),
        Err(ObjectNamespaceError::Precondition(_)) => {
            let existing = read_exact(namespace, key, maximum_bytes).await?;
            if existing == body {
                Ok(())
            } else {
                Err(corrupt(
                    "object namespace replay found different bytes at an immutable recovery key",
                ))
            }
        }
        Err(error) => Err(error),
    }
}

fn require_subset(
    observed: &[RecoveryManifestEntry],
    expected: &[RecoveryManifestEntry],
) -> Result<(), ObjectNamespaceError> {
    for entry in observed {
        match expected.binary_search_by(|candidate| candidate.key.as_str().cmp(entry.key.as_str()))
        {
            Ok(index) if expected[index] == *entry => {}
            _ => {
                return Err(corrupt(
                    "object namespace contains state outside the exact recovery manifest",
                ))
            }
        }
    }
    Ok(())
}

fn require_complete(
    observed: &StateObservation,
    manifest: &RecoveryManifest,
) -> Result<(), ObjectNamespaceError> {
    if observed.entries != manifest.entries
        || observed.digest != manifest.state_digest
        || observed.size_bytes != manifest.state_size_bytes
    {
        return Err(corrupt(
            "object namespace does not exactly match the sealed recovery manifest",
        ));
    }
    Ok(())
}

fn state_size(entries: &[RecoveryManifestEntry]) -> Result<u64, ObjectNamespaceError> {
    entries.iter().try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry.size_bytes)
            .ok_or_else(|| corrupt("object namespace recovery state size overflowed"))
    })
}

fn state_digest(
    entries: &[RecoveryManifestEntry],
    maximum_bytes: usize,
) -> Result<Sha256Digest, ObjectNamespaceError> {
    let bytes = canonical_json_bounded(
        &StateDigestProjection { entries },
        maximum_bytes,
        "object namespace recovery state",
    )
    .map_err(ObjectNamespaceError::Corrupt)?;
    Ok(Sha256Digest::from_bytes(&bytes))
}

fn manifest_bytes(
    manifest: &RecoveryManifest,
    maximum_bytes: usize,
) -> Result<Vec<u8>, ObjectNamespaceError> {
    canonical_json_bounded(
        manifest,
        maximum_bytes,
        "object namespace recovery manifest",
    )
    .map_err(ObjectNamespaceError::Corrupt)
}

fn receipt_digest<T: Serialize>(
    label: &str,
    projection: &T,
) -> Result<Sha256Digest, ObjectNamespaceError> {
    let bytes = canonical_json_bounded(projection, 32 * 1024, label)
        .map_err(ObjectNamespaceError::Corrupt)?;
    Ok(Sha256Digest::from_bytes(&bytes))
}

fn canonical_digest(digest: &Sha256Digest) -> Result<(), String> {
    if Sha256Digest::parse(digest.as_str())? != *digest {
        return Err("object namespace recovery digest is not canonical".into());
    }
    Ok(())
}

fn recovery_namespace_prefix(
    namespace_id: StorageNamespaceId,
) -> Result<ObjectNamespaceKey, ObjectNamespaceError> {
    ObjectNamespaceKey::parse(format!("points/{namespace_id}"))
        .map_err(ObjectNamespaceError::Invalid)
}

fn point_prefix(
    namespace_id: StorageNamespaceId,
    sequence: u64,
) -> Result<String, ObjectNamespaceError> {
    let prefix = format!("points/{namespace_id}/{sequence:020}");
    ObjectNamespaceKey::parse(prefix.clone()).map_err(ObjectNamespaceError::Invalid)?;
    Ok(prefix)
}

fn manifest_key(
    namespace_id: StorageNamespaceId,
    sequence: u64,
) -> Result<ObjectNamespaceKey, ObjectNamespaceError> {
    ObjectNamespaceKey::parse(format!(
        "{}/manifest.json",
        point_prefix(namespace_id, sequence)?
    ))
    .map_err(ObjectNamespaceError::Invalid)
}

fn snapshot_key(
    namespace_id: StorageNamespaceId,
    sequence: u64,
    source_key: &ObjectNamespaceKey,
) -> Result<ObjectNamespaceKey, ObjectNamespaceError> {
    ObjectNamespaceKey::parse(format!(
        "{}/objects/{}",
        point_prefix(namespace_id, sequence)?,
        source_key.as_str()
    ))
    .map_err(ObjectNamespaceError::Invalid)
}

fn deletion_marker_key(
    point: &ObjectNamespaceRecoveryPoint,
    deletion_plan: &ObjectNamespaceDeletionPlan,
) -> Result<ObjectNamespaceKey, ObjectNamespaceError> {
    let digest = deletion_plan
        .digest()
        .as_str()
        .strip_prefix("sha256:")
        .ok_or_else(|| invalid("object namespace deletion digest lost its algorithm"))?;
    ObjectNamespaceKey::parse(format!(
        "{}/delete-{digest}.json",
        point_prefix(point.spec().namespace_id, point.spec().sequence)?
    ))
    .map_err(ObjectNamespaceError::Invalid)
}

fn deletion_marker_bytes(
    point: &ObjectNamespaceRecoveryPoint,
    restore_plan: &ObjectNamespaceRestorePlan,
    deletion_plan: &ObjectNamespaceDeletionPlan,
    maximum_bytes: usize,
) -> Result<Vec<u8>, ObjectNamespaceError> {
    canonical_json_bounded(
        &DeletionMarkerProjection {
            schema: "a3s.s0.object-namespace.deletion-intent.v1",
            deletion_plan_digest: deletion_plan.digest(),
            recovery_point_digest: point.digest(),
            restore_plan_digest: restore_plan.digest(),
            source_namespace_id: point.spec().namespace_id,
            retained_restore_namespace_id: restore_plan.spec().target_namespace_id,
        },
        maximum_bytes,
        "object namespace deletion replay marker",
    )
    .map_err(ObjectNamespaceError::Corrupt)
}

fn invalid(message: impl Into<String>) -> ObjectNamespaceError {
    ObjectNamespaceError::Invalid(message.into())
}

fn corrupt(message: impl Into<String>) -> ObjectNamespaceError {
    ObjectNamespaceError::Corrupt(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::ImmutableObjectClient;
    use crate::modules::data::domain::{
        ObjectNamespaceRetentionPolicySpec, ObjectNamespaceVersion,
    };
    use async_trait::async_trait;
    use chrono::Duration;
    use object_store::memory::InMemory;
    use object_store::ObjectStore;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Clone, Copy)]
    enum FailureOperation {
        Create,
        Delete,
    }

    struct FailOnceNamespace {
        inner: Arc<dyn IObjectNamespace>,
        operation: FailureOperation,
        fail_at: usize,
        calls: AtomicUsize,
        failed: AtomicBool,
    }

    impl FailOnceNamespace {
        fn new(
            inner: Arc<dyn IObjectNamespace>,
            operation: FailureOperation,
            fail_at: usize,
        ) -> Self {
            Self {
                inner,
                operation,
                fail_at,
                calls: AtomicUsize::new(0),
                failed: AtomicBool::new(false),
            }
        }

        fn should_fail(&self, operation: FailureOperation) -> bool {
            std::mem::discriminant(&self.operation) == std::mem::discriminant(&operation)
                && self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_at
                && !self.failed.swap(true, Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl IObjectNamespace for FailOnceNamespace {
        async fn list(
            &self,
            key_prefix: Option<&ObjectNamespaceKey>,
            maximum_objects: u32,
            maximum_total_bytes: u64,
        ) -> Result<Vec<crate::modules::data::domain::ObjectNamespaceEntry>, ObjectNamespaceError>
        {
            self.inner
                .list(key_prefix, maximum_objects, maximum_total_bytes)
                .await
        }

        async fn conditional_create(
            &self,
            object_key: &ObjectNamespaceKey,
            body: Vec<u8>,
            maximum_bytes: u64,
        ) -> Result<ObjectNamespaceVersion, ObjectNamespaceError> {
            if self.should_fail(FailureOperation::Create) {
                return Err(ObjectNamespaceError::Unavailable(
                    "injected create interruption".into(),
                ));
            }
            self.inner
                .conditional_create(object_key, body, maximum_bytes)
                .await
        }

        async fn conditional_overwrite(
            &self,
            object_key: &ObjectNamespaceKey,
            expected: &ObjectNamespaceVersion,
            body: Vec<u8>,
            maximum_bytes: u64,
        ) -> Result<ObjectNamespaceVersion, ObjectNamespaceError> {
            self.inner
                .conditional_overwrite(object_key, expected, body, maximum_bytes)
                .await
        }

        async fn read(
            &self,
            object_key: &ObjectNamespaceKey,
            maximum_bytes: u64,
        ) -> Result<ObjectNamespaceRead, ObjectNamespaceError> {
            self.inner.read(object_key, maximum_bytes).await
        }

        async fn delete(
            &self,
            object_key: &ObjectNamespaceKey,
        ) -> Result<(), ObjectNamespaceError> {
            if self.should_fail(FailureOperation::Delete) {
                return Err(ObjectNamespaceError::Unavailable(
                    "injected delete interruption".into(),
                ));
            }
            self.inner.delete(object_key).await
        }
    }

    #[tokio::test]
    async fn shared_s0_executor_replays_restore_and_deletion_without_crossing_namespaces() {
        let objects = Arc::new(InMemory::new()) as Arc<dyn ObjectStore>;
        let source_id = StorageNamespaceId::new();
        let target_id = StorageNamespaceId::new();
        let profile = digest('a');
        let target_profile = digest('b');
        let source_namespace = namespace(&objects, "provider/source");
        let recovery_namespace = namespace(&objects, "provider/recovery");
        let target_namespace = namespace(&objects, "provider/target");
        let source =
            ObjectNamespaceAccess::new(source_id, profile.clone(), source_namespace.clone())
                .expect("source access");
        let recovery =
            ObjectNamespaceRecoveryStore::new(profile.clone(), recovery_namespace.clone())
                .expect("recovery access");
        let target =
            ObjectNamespaceAccess::new(target_id, target_profile.clone(), target_namespace.clone())
                .expect("target access");
        put(&source_namespace, "cells/alpha.sqlite", b"sqlite-state").await;
        put(&source_namespace, "alarms/alpha", b"alarm-state").await;
        put(&source_namespace, "websockets/alpha", b"socket-state").await;

        let executor =
            ObjectNamespaceRecoveryExecutor::new(32, 1024, 8192, 8192).expect("recovery executor");
        let now = canonical_timestamp(Utc::now());
        let point = executor
            .seal(&source, &recovery, None, 7, digest('c'), now)
            .await
            .expect("sealed point");
        assert_eq!(point.spec().namespace_id, source_id);
        assert_eq!(point.spec().sequence, 1);
        assert_eq!(
            executor
                .seal(&source, &recovery, None, 7, digest('c'), now,)
                .await
                .expect("exact seal replay"),
            point
        );

        let policy =
            ObjectNamespaceRetentionPolicy::from_spec(ObjectNamespaceRetentionPolicySpec {
                minimum_sealed_recovery_points: 1,
                maximum_sealed_recovery_points: 4,
                maximum_recovery_point_age_seconds: 24 * 60 * 60,
                deletion_grace_period_seconds: 5 * 60,
            })
            .expect("retention policy");
        let restore_plan = ObjectNamespaceRestorePlan::for_recovery_point(
            &point,
            target_id,
            target_profile,
            &policy,
            now + Duration::seconds(1),
        )
        .expect("restore plan");

        let interrupted_target = ObjectNamespaceAccess::new(
            target_id,
            restore_plan.spec().target_provider_profile_digest.clone(),
            Arc::new(FailOnceNamespace::new(
                target_namespace.clone(),
                FailureOperation::Create,
                2,
            )),
        )
        .expect("interrupted target");
        assert!(matches!(
            executor
                .restore(
                    &recovery,
                    &interrupted_target,
                    &point,
                    &restore_plan,
                    &policy,
                    now + Duration::seconds(2),
                )
                .await,
            Err(ObjectNamespaceError::Unavailable(_))
        ));
        let restore_evidence = executor
            .restore(
                &recovery,
                &target,
                &point,
                &restore_plan,
                &policy,
                now + Duration::seconds(2),
            )
            .await
            .expect("replayed isolated restore");
        restore_evidence
            .validate_for(&restore_plan)
            .expect("restore evidence");
        assert_eq!(
            executor
                .restore(
                    &recovery,
                    &target,
                    &point,
                    &restore_plan,
                    &policy,
                    now + Duration::seconds(2),
                )
                .await
                .expect("exact restore replay"),
            restore_evidence
        );

        let foreign_target_namespace = namespace(&objects, "provider/foreign-target");
        let foreign_target = ObjectNamespaceAccess::new(
            StorageNamespaceId::new(),
            restore_plan.spec().target_provider_profile_digest.clone(),
            foreign_target_namespace.clone(),
        )
        .expect("foreign target");
        assert!(matches!(
            executor
                .restore(
                    &recovery,
                    &foreign_target,
                    &point,
                    &restore_plan,
                    &policy,
                    now + Duration::seconds(2),
                )
                .await,
            Err(ObjectNamespaceError::Invalid(_))
        ));
        assert!(foreign_target_namespace
            .list(None, 10, 1024)
            .await
            .expect("foreign target list")
            .is_empty());

        let deletion_plan = ObjectNamespaceDeletionPlan::after_verified_restore(
            &point,
            &restore_plan,
            &restore_evidence,
            &policy,
            digest('d'),
            digest('e'),
            now + Duration::seconds(3),
        )
        .expect("deletion plan");
        assert!(matches!(
            executor
                .delete(
                    &source,
                    &recovery,
                    &target,
                    &point,
                    &restore_plan,
                    &restore_evidence,
                    &deletion_plan,
                    &policy,
                    deletion_plan.spec().not_before - Duration::microseconds(1),
                )
                .await,
            Err(ObjectNamespaceError::Precondition(_))
        ));
        assert_eq!(
            source_namespace
                .list(None, 32, 8192)
                .await
                .expect("source before grace")
                .len(),
            3
        );

        let missing_key = ObjectNamespaceKey::parse("alarms/alpha").expect("missing key");
        source_namespace
            .delete(&missing_key)
            .await
            .expect("simulate pre-operation state loss");
        assert!(matches!(
            executor
                .delete(
                    &source,
                    &recovery,
                    &target,
                    &point,
                    &restore_plan,
                    &restore_evidence,
                    &deletion_plan,
                    &policy,
                    deletion_plan.spec().not_before,
                )
                .await,
            Err(ObjectNamespaceError::Corrupt(_))
        ));
        put(&source_namespace, "alarms/alpha", b"alarm-state").await;

        let interrupted_source = ObjectNamespaceAccess::new(
            source_id,
            profile,
            Arc::new(FailOnceNamespace::new(
                source_namespace.clone(),
                FailureOperation::Delete,
                2,
            )),
        )
        .expect("interrupted source");
        assert!(matches!(
            executor
                .delete(
                    &interrupted_source,
                    &recovery,
                    &target,
                    &point,
                    &restore_plan,
                    &restore_evidence,
                    &deletion_plan,
                    &policy,
                    deletion_plan.spec().not_before,
                )
                .await,
            Err(ObjectNamespaceError::Unavailable(_))
        ));
        let deletion_evidence = executor
            .delete(
                &source,
                &recovery,
                &target,
                &point,
                &restore_plan,
                &restore_evidence,
                &deletion_plan,
                &policy,
                deletion_plan.spec().not_before,
            )
            .await
            .expect("replayed deletion");
        deletion_evidence
            .validate_for(&deletion_plan)
            .expect("deletion evidence");
        assert!(source_namespace
            .list(None, 32, 8192)
            .await
            .expect("empty source")
            .is_empty());
        assert!(recovery_namespace
            .list(None, 256, 65536)
            .await
            .expect("empty recovery")
            .is_empty());
        assert_eq!(
            executor
                .delete(
                    &source,
                    &recovery,
                    &target,
                    &point,
                    &restore_plan,
                    &restore_evidence,
                    &deletion_plan,
                    &policy,
                    deletion_plan.spec().not_before,
                )
                .await
                .expect("exact deletion replay"),
            deletion_evidence
        );
        assert_eq!(
            target_namespace
                .list(None, 32, 8192)
                .await
                .expect("retained target")
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn sealed_successor_replay_binds_the_exact_predecessor_before_side_effects() {
        let objects = Arc::new(InMemory::new()) as Arc<dyn ObjectStore>;
        let namespace_id = StorageNamespaceId::new();
        let profile = digest('a');
        let source_namespace = namespace(&objects, "provider/lineage-source");
        let recovery_namespace = namespace(&objects, "provider/lineage-recovery");
        let source =
            ObjectNamespaceAccess::new(namespace_id, profile.clone(), source_namespace.clone())
                .expect("source");
        let recovery = ObjectNamespaceRecoveryStore::new(profile, recovery_namespace.clone())
            .expect("recovery");
        put(&source_namespace, "state", b"one").await;
        let executor = ObjectNamespaceRecoveryExecutor::new(8, 1024, 4096, 4096).expect("executor");
        let now = canonical_timestamp(Utc::now());
        let first = executor
            .seal(&source, &recovery, None, 1, digest('b'), now)
            .await
            .expect("first point");
        let second = executor
            .seal(
                &source,
                &recovery,
                Some(&first),
                2,
                digest('c'),
                now + Duration::seconds(1),
            )
            .await
            .expect("second point");

        let alternate_previous =
            ObjectNamespaceRecoveryPoint::seal(ObjectNamespaceRecoveryPointSpec {
                state_digest: digest('d'),
                ..first.spec().clone()
            })
            .expect("alternate predecessor");
        assert_ne!(alternate_previous.digest(), first.digest());
        assert!(matches!(
            executor
                .seal(
                    &source,
                    &recovery,
                    Some(&alternate_previous),
                    2,
                    digest('c'),
                    now + Duration::seconds(1),
                )
                .await,
            Err(ObjectNamespaceError::Corrupt(_))
        ));
        assert!(matches!(
            executor
                .seal(
                    &source,
                    &recovery,
                    Some(&second),
                    1,
                    digest('e'),
                    now + Duration::seconds(2),
                )
                .await,
            Err(ObjectNamespaceError::Invalid(_))
        ));
        assert_eq!(
            recovery_namespace
                .list(None, 16, 16 * 1024)
                .await
                .expect("two points only")
                .len(),
            4
        );
    }

    #[test]
    fn recovery_executor_reuses_s0_and_owning_workflow_boundaries() {
        let source = include_str!("object_namespace_recovery.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for forbidden in [
            "object_store::",
            "ImmutableObjectClient",
            "IFlowClient",
            "tokio::spawn",
            "std::fs::",
            "reqwest::",
            "IWorkloadRepository",
        ] {
            assert!(
                !production.contains(forbidden),
                "S0 recovery execution must reuse owning mechanisms; found {forbidden}"
            );
        }
    }

    fn namespace(objects: &Arc<dyn ObjectStore>, prefix: &str) -> Arc<dyn IObjectNamespace> {
        Arc::new(
            ImmutableObjectClient::from_store(Arc::clone(objects), prefix)
                .expect("shared object namespace"),
        )
    }

    async fn put(namespace: &Arc<dyn IObjectNamespace>, key: &str, body: &[u8]) {
        namespace
            .conditional_create(
                &ObjectNamespaceKey::parse(key).expect("object key"),
                body.to_vec(),
                1024,
            )
            .await
            .expect("object create");
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }
}
