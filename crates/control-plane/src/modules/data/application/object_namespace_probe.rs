use crate::modules::data::domain::{
    IObjectNamespace, ObjectNamespaceError, ObjectNamespaceKey, ObjectNamespaceProbeEvidence,
    ObjectNamespaceRead,
};
use crate::modules::shared_kernel::domain::StorageNamespaceId;
use std::sync::Arc;
use uuid::Uuid;

const MINIMUM_PROBE_BOUND_BYTES: u64 = 128;

/// Destructive startup certification for one already scoped S0 namespace.
///
/// Every key is unique to this run and is removed before success is returned.
/// A provider that cannot prove create-only writes, version-bound overwrite,
/// read-after-write, stale-token rejection, and cleanup remains unavailable.
#[derive(Clone)]
pub struct ObjectNamespaceConformanceProbe {
    namespace: Arc<dyn IObjectNamespace>,
    maximum_probe_bytes: u64,
}

impl ObjectNamespaceConformanceProbe {
    pub fn new(
        namespace: Arc<dyn IObjectNamespace>,
        maximum_probe_bytes: u64,
    ) -> Result<Self, String> {
        if maximum_probe_bytes < MINIMUM_PROBE_BOUND_BYTES {
            return Err("object namespace probe bound is too small".into());
        }
        Ok(Self {
            namespace,
            maximum_probe_bytes,
        })
    }

    pub async fn run(
        &self,
        namespace_id: StorageNamespaceId,
    ) -> Result<ObjectNamespaceProbeEvidence, ObjectNamespaceError> {
        if namespace_id.as_uuid().is_nil() {
            return Err(ObjectNamespaceError::Invalid(
                "storage namespace identity is nil".into(),
            ));
        }
        let probe_id = Uuid::now_v7();
        let key = ObjectNamespaceKey::parse(format!(".a3s-conformance/cas/{probe_id}"))
            .map_err(ObjectNamespaceError::Invalid)?;
        let initial = format!("a3s-s0-cas-v1:{probe_id}:initial").into_bytes();
        let successor = format!("a3s-s0-cas-v1:{probe_id}:successor").into_bytes();
        let stale = format!("a3s-s0-cas-v1:{probe_id}:stale").into_bytes();

        let probe = self
            .run_sequence(namespace_id, &key, initial, successor, stale)
            .await;
        let cleanup = self.namespace.delete(&key).await;
        let cleanup_read = match &cleanup {
            Ok(()) => self.namespace.read(&key, self.maximum_probe_bytes).await,
            Err(_) => Ok(ObjectNamespaceRead::Corrupt),
        };

        match (probe, cleanup, cleanup_read) {
            (Ok(mut evidence), Ok(()), Ok(ObjectNamespaceRead::Missing)) => {
                evidence.cleanup_verified = true;
                evidence.validate().map_err(ObjectNamespaceError::Corrupt)?;
                Ok(evidence)
            }
            (Err(error), Ok(()), Ok(ObjectNamespaceRead::Missing)) => Err(error),
            (probe, cleanup, cleanup_read) => Err(ObjectNamespaceError::Unavailable(format!(
                "object namespace conformance cleanup failed (probe={}, delete={}, read={})",
                result_kind(&probe),
                result_kind(&cleanup),
                result_kind(&cleanup_read)
            ))),
        }
    }

    async fn run_sequence(
        &self,
        namespace_id: StorageNamespaceId,
        key: &ObjectNamespaceKey,
        initial: Vec<u8>,
        successor: Vec<u8>,
        stale: Vec<u8>,
    ) -> Result<ObjectNamespaceProbeEvidence, ObjectNamespaceError> {
        let initial_version = self
            .namespace
            .conditional_create(key, initial.clone(), self.maximum_probe_bytes)
            .await?;
        match self
            .namespace
            .conditional_create(key, stale.clone(), self.maximum_probe_bytes)
            .await
        {
            Err(ObjectNamespaceError::Precondition(_)) => {}
            Ok(_) => {
                return Err(ObjectNamespaceError::Corrupt(
                    "competing conditional create replaced an existing object".into(),
                ))
            }
            Err(error) => return Err(error),
        }
        require_read(
            self.namespace.read(key, self.maximum_probe_bytes).await?,
            &initial,
            &initial_version,
            "create",
        )?;

        let successor_version = self
            .namespace
            .conditional_overwrite(
                key,
                &initial_version,
                successor.clone(),
                self.maximum_probe_bytes,
            )
            .await?;
        if successor_version == initial_version {
            return Err(ObjectNamespaceError::Corrupt(
                "conditional overwrite did not advance the provider token".into(),
            ));
        }
        match self
            .namespace
            .conditional_overwrite(key, &initial_version, stale, self.maximum_probe_bytes)
            .await
        {
            Err(ObjectNamespaceError::Precondition(_)) => {}
            Ok(_) => {
                return Err(ObjectNamespaceError::Corrupt(
                    "stale conditional overwrite entered the active lineage".into(),
                ))
            }
            Err(error) => return Err(error),
        }
        require_read(
            self.namespace.read(key, self.maximum_probe_bytes).await?,
            &successor,
            &successor_version,
            "overwrite",
        )?;

        Ok(ObjectNamespaceProbeEvidence {
            namespace_id,
            conditional_create: true,
            competing_create_rejected: true,
            read_after_create: true,
            conditional_overwrite: true,
            stale_overwrite_rejected: true,
            read_after_overwrite: true,
            cleanup_verified: false,
        })
    }
}

fn require_read(
    observed: ObjectNamespaceRead,
    expected_body: &[u8],
    expected_version: &crate::modules::data::domain::ObjectNamespaceVersion,
    phase: &str,
) -> Result<(), ObjectNamespaceError> {
    match observed {
        ObjectNamespaceRead::Found { body, version }
            if body == expected_body && &version == expected_version =>
        {
            Ok(())
        }
        _ => Err(ObjectNamespaceError::Corrupt(format!(
            "read-after-{phase} did not return the accepted body and version"
        ))),
    }
}

fn result_kind<T, E>(result: &Result<T, E>) -> &'static str {
    if result.is_ok() {
        "ok"
    } else {
        "error"
    }
}
