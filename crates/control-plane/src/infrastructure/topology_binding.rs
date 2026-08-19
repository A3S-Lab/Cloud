use super::{execute, fetch_optional, InfrastructureBindings, PostgresPersistenceError};
use a3s_orm::{
    insert_into, select_from, PostgresError, PostgresExecutor, PostgresTransactionError,
};
use chrono::Utc;
use sha2::{Digest, Sha256};

const MAX_BINDING_NAME_BYTES: usize = 128;
const MAX_BINDING_SCHEMA_BYTES: usize = 128;
const MAX_BINDING_IDENTITY_BYTES: usize = 16 * 1024;
const LOCK_SCOPE: &str = "a3s.cloud.infrastructure-binding";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InfrastructureBinding {
    name: String,
    schema: String,
    digest: String,
}

impl InfrastructureBinding {
    pub(crate) fn new(
        name: &str,
        schema: &str,
        canonical_identity: &[u8],
    ) -> Result<Self, InfrastructureBindingError> {
        if !valid_name(name)
            || schema.len() > MAX_BINDING_SCHEMA_BYTES
            || !valid_schema(schema)
            || canonical_identity.is_empty()
            || canonical_identity.len() > MAX_BINDING_IDENTITY_BYTES
        {
            return Err(InfrastructureBindingError::Invalid(
                "infrastructure binding identity is invalid or exceeds its bound".into(),
            ));
        }
        Ok(Self {
            name: name.into(),
            schema: schema.into(),
            digest: format!("sha256:{:x}", Sha256::digest(canonical_identity)),
        })
    }

    #[cfg(test)]
    fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum InfrastructureBindingError {
    #[error("invalid infrastructure binding: {0}")]
    Invalid(String),
    #[error("infrastructure binding {0:?} conflicts with this PostgreSQL deployment")]
    Conflict(String),
    #[error("could not persist infrastructure binding: {0}")]
    Storage(String),
}

#[derive(Debug, thiserror::Error)]
enum BindingTransactionError {
    #[error(transparent)]
    Persistence(#[from] PostgresPersistenceError),
    #[error(transparent)]
    Database(#[from] PostgresError),
    #[error("binding changed identity")]
    Conflict,
}

pub(crate) async fn bind_infrastructure(
    executor: &PostgresExecutor,
    binding: InfrastructureBinding,
) -> Result<(), InfrastructureBindingError> {
    let conflict_name = binding.name.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                transaction
                    .advisory_xact_lock(LOCK_SCOPE, &binding.name)
                    .await?;
                let existing = fetch_optional::<(String, String), _>(
                    transaction,
                    select_from::<InfrastructureBindings>()
                        .select((
                            InfrastructureBindings::binding_schema(),
                            InfrastructureBindings::binding_digest(),
                        ))
                        .filter(InfrastructureBindings::binding_name().eq(binding.name.as_str())),
                )
                .await?;
                match existing {
                    Some((schema, digest))
                        if schema == binding.schema && digest == binding.digest =>
                    {
                        Ok(())
                    }
                    Some(_) => Err(BindingTransactionError::Conflict),
                    None => {
                        let rows = execute(
                            transaction,
                            insert_into::<InfrastructureBindings>()
                                .value(
                                    InfrastructureBindings::binding_name(),
                                    binding.name.as_str(),
                                )
                                .value(
                                    InfrastructureBindings::binding_schema(),
                                    binding.schema.as_str(),
                                )
                                .value(
                                    InfrastructureBindings::binding_digest(),
                                    binding.digest.as_str(),
                                )
                                .value(InfrastructureBindings::bound_at(), Utc::now()),
                        )
                        .await?;
                        if rows == 1 {
                            Ok(())
                        } else {
                            Err(PostgresPersistenceError::Invariant(format!(
                                "writing infrastructure binding {:?} affected {rows} rows",
                                binding.name
                            ))
                            .into())
                        }
                    }
                }
            })
        })
        .await
        .map_err(|error| match error {
            PostgresTransactionError::Operation(BindingTransactionError::Conflict) => {
                InfrastructureBindingError::Conflict(conflict_name)
            }
            error => InfrastructureBindingError::Storage(error.to_string()),
        })
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_BINDING_NAME_BYTES
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || (index > 0 && (byte.is_ascii_digit() || byte == b'-'))
        })
}

fn valid_schema(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("a3s.cloud.") else {
        return false;
    };
    let Some((subject, version)) = rest.rsplit_once(".v") else {
        return false;
    };
    !subject.is_empty()
        && subject.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
        && !subject.starts_with('.')
        && !subject.ends_with('.')
        && !subject.contains("..")
        && !version.is_empty()
        && !version.starts_with('0')
        && version.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_identity_is_bounded_canonical_and_secret_opaque() {
        let first = InfrastructureBinding::new(
            "object-storage",
            "a3s.cloud.object-storage-topology.v1",
            b"provider=s3\0bucket=objects-a",
        )
        .expect("binding");
        let replay = InfrastructureBinding::new(
            "object-storage",
            "a3s.cloud.object-storage-topology.v1",
            b"provider=s3\0bucket=objects-a",
        )
        .expect("binding replay");
        let changed = InfrastructureBinding::new(
            "object-storage",
            "a3s.cloud.object-storage-topology.v1",
            b"provider=s3\0bucket=objects-b",
        )
        .expect("changed binding");
        assert_eq!(first.digest(), replay.digest());
        assert_ne!(first.digest(), changed.digest());
        assert_eq!(first.digest().len(), 71);
        assert!(!first.digest().contains("objects-a"));
    }

    #[test]
    fn binding_names_schemas_and_payloads_are_closed() {
        for (name, schema, identity) in [
            (
                "ObjectStorage",
                "a3s.cloud.object-storage.v1",
                b"x".as_slice(),
            ),
            (
                "object_storage",
                "a3s.cloud.object-storage.v1",
                b"x".as_slice(),
            ),
            (
                "object-storage",
                "a3s.cloud.object-storage.v0",
                b"x".as_slice(),
            ),
            ("object-storage", "cloud.object-storage.v1", b"x".as_slice()),
            (
                "object-storage",
                "a3s.cloud.object-storage.v1",
                b"".as_slice(),
            ),
        ] {
            assert!(InfrastructureBinding::new(name, schema, identity).is_err());
        }
        assert!(InfrastructureBinding::new(
            "object-storage",
            "a3s.cloud.object-storage.v1",
            &vec![0; MAX_BINDING_IDENTITY_BYTES + 1],
        )
        .is_err());
    }

    #[tokio::test]
    async fn postgres_binding_is_create_only_exact_and_conflict_detecting_when_available() {
        let Ok(url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL") else {
            return;
        };
        crate::infrastructure::migrate_postgres(&url, 2)
            .await
            .expect("migrate PostgreSQL");
        let executor = crate::infrastructure::connect_postgres(&url, 2)
            .await
            .expect("connect PostgreSQL");
        let name = format!("test-{}", uuid::Uuid::now_v7());
        let first = InfrastructureBinding::new(&name, "a3s.cloud.test-topology.v1", b"provider-a")
            .expect("first binding");
        bind_infrastructure(&executor, first.clone())
            .await
            .expect("first write");
        bind_infrastructure(&executor, first)
            .await
            .expect("exact replay");
        let changed =
            InfrastructureBinding::new(&name, "a3s.cloud.test-topology.v1", b"provider-b")
                .expect("changed binding");
        assert!(matches!(
            bind_infrastructure(&executor, changed).await,
            Err(InfrastructureBindingError::Conflict(conflict)) if conflict == name
        ));
    }
}
