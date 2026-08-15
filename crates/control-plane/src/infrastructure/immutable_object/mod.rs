mod local;
mod s3;
#[cfg(test)]
mod s3_test;
mod stream;

#[cfg(test)]
mod tests;

use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, PutMode, PutResult, UpdateVersion};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) use s3::S3ImmutableObjectOptions;
#[cfg(test)]
pub(crate) use s3_test::DisposableS3TestContext;
pub(crate) use stream::{
    ImmutableObjectOpenResult, ImmutableObjectReader, ImmutableObjectVerification,
};

const MAX_OBJECT_PATH_BYTES: usize = 4096;
const MAX_CONDITIONAL_VERSION_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImmutableObjectWrite {
    pub(crate) created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConditionalObjectVersion {
    e_tag: Option<String>,
    version: Option<String>,
}

impl ConditionalObjectVersion {
    fn from_put(result: PutResult) -> Result<Self, ConditionalObjectError> {
        Self::new(result.e_tag, result.version)
    }

    fn new(e_tag: Option<String>, version: Option<String>) -> Result<Self, ConditionalObjectError> {
        if e_tag.as_deref().is_some_and(invalid_conditional_token)
            || version.as_deref().is_some_and(invalid_conditional_token)
            || e_tag.is_none() && version.is_none()
        {
            return Err(ConditionalObjectError::Unsupported(
                "object provider returned no usable conditional-write version".into(),
            ));
        }
        Ok(Self { e_tag, version })
    }

    pub(crate) fn from_parts(
        e_tag: Option<String>,
        version: Option<String>,
    ) -> Result<Self, ConditionalObjectError> {
        Self::new(e_tag, version)
    }

    pub(crate) fn e_tag(&self) -> Option<&str> {
        self.e_tag.as_deref()
    }

    pub(crate) fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    fn update_version(&self) -> UpdateVersion {
        UpdateVersion {
            e_tag: self.e_tag.clone(),
            version: self.version.clone(),
        }
    }
}

fn invalid_conditional_token(value: &str) -> bool {
    value.is_empty()
        || value.len() > MAX_CONDITIONAL_VERSION_BYTES
        || value.contains(['\0', '\r', '\n'])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConditionalObjectWrite {
    pub(crate) version: ConditionalObjectVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConditionalObjectRead {
    Found {
        body: Vec<u8>,
        version: ConditionalObjectVersion,
    },
    Missing,
    Corrupt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConditionalObjectError {
    #[error("object namespace conditional request is invalid: {0}")]
    Invalid(String),
    #[error("object namespace conditional request lost its precondition: {0}")]
    Precondition(String),
    #[error("object namespace conditional content is corrupt: {0}")]
    Corrupt(String),
    #[error("object namespace conditional capability is unsupported: {0}")]
    Unsupported(String),
    #[error("object namespace conditional storage is unavailable: {0}")]
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImmutableObjectRead {
    Found(Vec<u8>),
    Missing,
    Corrupt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ImmutableObjectError {
    #[error("immutable object request is invalid: {0}")]
    Invalid(String),
    #[error("immutable object conflicts with existing content: {0}")]
    Conflict(String),
    #[error("immutable object failed integrity validation: {0}")]
    Integrity(String),
    #[error("immutable object storage is unavailable: {0}")]
    Unavailable(String),
}

#[derive(Clone)]
pub(crate) struct ImmutableObjectClient {
    backend: Arc<Backend>,
    namespace: String,
}

enum Backend {
    Local(local::LocalBackend),
    Remote(Arc<dyn ObjectStore>),
}

impl fmt::Debug for ImmutableObjectClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backend = match self.backend.as_ref() {
            Backend::Local(_) => "local",
            Backend::Remote(_) => "remote",
        };
        formatter
            .debug_struct("ImmutableObjectClient")
            .field("backend", &backend)
            .field("namespace", &self.namespace)
            .finish()
    }
}

impl ImmutableObjectClient {
    pub(crate) fn local(
        root: impl Into<PathBuf>,
        namespace: &str,
    ) -> Result<Self, ImmutableObjectError> {
        validate_relative_path(namespace, "namespace")?;
        Ok(Self {
            backend: Arc::new(Backend::Local(local::LocalBackend::new(root.into())?)),
            namespace: namespace.into(),
        })
    }

    pub(crate) fn s3(options: S3ImmutableObjectOptions) -> Result<Self, ImmutableObjectError> {
        validate_relative_path(&options.prefix, "namespace")?;
        let namespace = options.prefix.clone();
        Ok(Self {
            backend: Arc::new(Backend::Remote(s3::build(options)?)),
            namespace,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_store(
        objects: Arc<dyn ObjectStore>,
        namespace: &str,
    ) -> Result<Self, ImmutableObjectError> {
        validate_relative_path(namespace, "namespace")?;
        Ok(Self {
            backend: Arc::new(Backend::Remote(objects)),
            namespace: namespace.into(),
        })
    }

    pub(crate) async fn put(
        &self,
        object_key: &str,
        body: Vec<u8>,
        maximum_bytes: u64,
    ) -> Result<ImmutableObjectWrite, ImmutableObjectError> {
        let scoped_key = self.scoped_key(object_key)?;
        if maximum_bytes == 0 || body.len() as u64 > maximum_bytes {
            return Err(ImmutableObjectError::Invalid(
                "immutable object exceeds its admission bound".into(),
            ));
        }
        match self.backend.as_ref() {
            Backend::Local(backend) => {
                let backend = backend.clone();
                let conflict_key = object_key.to_owned();
                tokio::task::spawn_blocking(move || backend.put(&scoped_key, &conflict_key, &body))
                    .await
                    .map_err(join_error("writer"))?
            }
            Backend::Remote(objects) => {
                let path = remote_path(&scoped_key)?;
                let created = match objects
                    .put_opts(&path, body.clone().into(), PutMode::Create.into())
                    .await
                {
                    Ok(_) => true,
                    Err(object_store::Error::AlreadyExists { .. }) => {
                        if remote_existing_matches(objects, &path, &body).await? {
                            false
                        } else {
                            return Err(ImmutableObjectError::Conflict(object_key.into()));
                        }
                    }
                    Err(error) => return Err(remote_error("write immutable object", error)),
                };
                Ok(ImmutableObjectWrite { created })
            }
        }
    }

    pub(crate) async fn get(
        &self,
        object_key: &str,
        maximum_bytes: u64,
    ) -> Result<ImmutableObjectRead, ImmutableObjectError> {
        let scoped_key = self.scoped_key(object_key)?;
        if maximum_bytes == 0 {
            return Err(ImmutableObjectError::Invalid(
                "immutable object read bound must be positive".into(),
            ));
        }
        match self.backend.as_ref() {
            Backend::Local(backend) => {
                let backend = backend.clone();
                tokio::task::spawn_blocking(move || backend.get(&scoped_key, maximum_bytes))
                    .await
                    .map_err(join_error("reader"))?
            }
            Backend::Remote(objects) => {
                read_remote(objects, &remote_path(&scoped_key)?, maximum_bytes).await
            }
        }
    }

    /// Atomically creates one mutable object only when the key is absent.
    ///
    /// This deliberately shares the same backend and namespace isolation as
    /// immutable objects. Typed S0 adapters decide which keys may use mutable
    /// conditional semantics; existing immutable adapters still call `put`.
    pub(crate) async fn conditional_create(
        &self,
        object_key: &str,
        body: Vec<u8>,
        maximum_bytes: u64,
    ) -> Result<ConditionalObjectWrite, ConditionalObjectError> {
        let scoped_key = self
            .scoped_key(object_key)
            .map_err(map_base_conditional_error)?;
        validate_conditional_body(&body, maximum_bytes)?;
        let Backend::Remote(objects) = self.backend.as_ref() else {
            return Err(ConditionalObjectError::Unsupported(
                "the local immutable-object backend is not certified for atomic compare-and-swap"
                    .into(),
            ));
        };
        let path = remote_path(&scoped_key).map_err(map_base_conditional_error)?;
        let result = objects
            .put_opts(&path, body.into(), PutMode::Create.into())
            .await
            .map_err(|error| conditional_remote_error("conditionally create object", error))?;
        Ok(ConditionalObjectWrite {
            version: ConditionalObjectVersion::from_put(result)?,
        })
    }

    /// Atomically replaces one mutable object only when its opaque provider
    /// version still matches the version observed by the caller.
    pub(crate) async fn conditional_overwrite(
        &self,
        object_key: &str,
        expected: &ConditionalObjectVersion,
        body: Vec<u8>,
        maximum_bytes: u64,
    ) -> Result<ConditionalObjectWrite, ConditionalObjectError> {
        let scoped_key = self
            .scoped_key(object_key)
            .map_err(map_base_conditional_error)?;
        validate_conditional_body(&body, maximum_bytes)?;
        ConditionalObjectVersion::new(expected.e_tag.clone(), expected.version.clone())?;
        let Backend::Remote(objects) = self.backend.as_ref() else {
            return Err(ConditionalObjectError::Unsupported(
                "the local immutable-object backend is not certified for atomic compare-and-swap"
                    .into(),
            ));
        };
        let path = remote_path(&scoped_key).map_err(map_base_conditional_error)?;
        let result = objects
            .put_opts(
                &path,
                body.into(),
                PutMode::Update(expected.update_version()).into(),
            )
            .await
            .map_err(|error| conditional_remote_error("conditionally overwrite object", error))?;
        Ok(ConditionalObjectWrite {
            version: ConditionalObjectVersion::from_put(result)?,
        })
    }

    /// Reads the body and the exact provider version from one response so the
    /// token cannot be confused with a later observation.
    pub(crate) async fn conditional_get(
        &self,
        object_key: &str,
        maximum_bytes: u64,
    ) -> Result<ConditionalObjectRead, ConditionalObjectError> {
        let scoped_key = self
            .scoped_key(object_key)
            .map_err(map_base_conditional_error)?;
        if maximum_bytes == 0 {
            return Err(ConditionalObjectError::Invalid(
                "conditional object read bound must be positive".into(),
            ));
        }
        let Backend::Remote(objects) = self.backend.as_ref() else {
            return Err(ConditionalObjectError::Unsupported(
                "the local immutable-object backend is not certified for atomic compare-and-swap"
                    .into(),
            ));
        };
        let path = remote_path(&scoped_key).map_err(map_base_conditional_error)?;
        let result = match objects.get(&path).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(ConditionalObjectRead::Missing),
            Err(error) => return Err(conditional_remote_error("read conditional object", error)),
        };
        if result.meta.size > maximum_bytes {
            return Ok(ConditionalObjectRead::Corrupt);
        }
        let version = ConditionalObjectVersion::from_parts(
            result.meta.e_tag.clone(),
            result.meta.version.clone(),
        )?;
        let body = result
            .bytes()
            .await
            .map_err(|error| conditional_remote_error("collect conditional object", error))?;
        if body.len() as u64 > maximum_bytes {
            return Ok(ConditionalObjectRead::Corrupt);
        }
        Ok(ConditionalObjectRead::Found {
            body: body.to_vec(),
            version,
        })
    }

    pub(crate) async fn remove(&self, object_key: &str) -> Result<(), ImmutableObjectError> {
        let scoped_key = self.scoped_key(object_key)?;
        match self.backend.as_ref() {
            Backend::Local(backend) => {
                let backend = backend.clone();
                tokio::task::spawn_blocking(move || backend.remove(&scoped_key))
                    .await
                    .map_err(join_error("remover"))?
            }
            Backend::Remote(objects) => match objects.delete(&remote_path(&scoped_key)?).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(error) => Err(remote_error("delete immutable object", error)),
            },
        }
    }

    /// Test-only corruption hook over the same already-built remote client.
    ///
    /// Real-provider tests use this to prove typed immutable readers reject an
    /// out-of-band overwrite without constructing a second S3 client.
    #[cfg(test)]
    pub(crate) async fn overwrite_remote_for_test(
        &self,
        object_key: &str,
        body: Vec<u8>,
    ) -> Result<(), ImmutableObjectError> {
        let scoped_key = self.scoped_key(object_key)?;
        let Backend::Remote(objects) = self.backend.as_ref() else {
            return Err(ImmutableObjectError::Invalid(
                "test overwrite requires the shared remote object client".into(),
            ));
        };
        objects
            .put(&remote_path(&scoped_key)?, body.into())
            .await
            .map(|_| ())
            .map_err(|error| remote_error("overwrite test object", error))
    }

    pub(crate) async fn health(&self) -> Result<bool, ImmutableObjectError> {
        let key = format!(".health/{}", Uuid::now_v7());
        self.put(&key, b"ok".to_vec(), 2).await?;
        let read = self.get(&key, 2).await;
        let removed = self.remove(&key).await;
        match (read, removed) {
            (Ok(ImmutableObjectRead::Found(body)), Ok(())) if body == b"ok" => Ok(true),
            (Ok(_), Ok(())) => Err(ImmutableObjectError::Unavailable(
                "immutable object health probe changed after write".into(),
            )),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(read_error), Err(remove_error)) => Err(ImmutableObjectError::Unavailable(
                format!("{read_error}; cleanup also failed: {remove_error}"),
            )),
        }
    }

    fn scoped_key(&self, object_key: &str) -> Result<String, ImmutableObjectError> {
        validate_relative_path(object_key, "object key")?;
        Ok(format!("{}/{object_key}", self.namespace))
    }
}

async fn remote_existing_matches(
    objects: &Arc<dyn ObjectStore>,
    path: &ObjectPath,
    expected: &[u8],
) -> Result<bool, ImmutableObjectError> {
    match read_remote(objects, path, expected.len() as u64).await? {
        ImmutableObjectRead::Found(body) => Ok(body == expected),
        ImmutableObjectRead::Missing | ImmutableObjectRead::Corrupt => Ok(false),
    }
}

async fn read_remote(
    objects: &Arc<dyn ObjectStore>,
    path: &ObjectPath,
    maximum_bytes: u64,
) -> Result<ImmutableObjectRead, ImmutableObjectError> {
    let result = match objects.get(path).await {
        Ok(result) => result,
        Err(object_store::Error::NotFound { .. }) => return Ok(ImmutableObjectRead::Missing),
        Err(error) => return Err(remote_error("read immutable object", error)),
    };
    if result.meta.size > maximum_bytes {
        return Ok(ImmutableObjectRead::Corrupt);
    }
    let body = result
        .bytes()
        .await
        .map_err(|error| remote_error("collect immutable object", error))?;
    Ok(ImmutableObjectRead::Found(body.to_vec()))
}

fn validate_relative_path(value: &str, description: &str) -> Result<(), ImmutableObjectError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > MAX_OBJECT_PATH_BYTES
        || value.contains(['\\', '\0', '\r', '\n'])
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ImmutableObjectError::Invalid(format!(
            "immutable object {description} is invalid"
        )));
    }
    Ok(())
}

fn validate_conditional_body(
    body: &[u8],
    maximum_bytes: u64,
) -> Result<(), ConditionalObjectError> {
    if maximum_bytes == 0 || body.len() as u64 > maximum_bytes {
        return Err(ConditionalObjectError::Invalid(
            "conditional object exceeds its admission bound".into(),
        ));
    }
    Ok(())
}

fn remote_path(scoped_key: &str) -> Result<ObjectPath, ImmutableObjectError> {
    ObjectPath::parse(scoped_key).map_err(|error| ImmutableObjectError::Invalid(error.to_string()))
}

fn remote_error(action: &str, error: object_store::Error) -> ImmutableObjectError {
    ImmutableObjectError::Unavailable(format!("{action}: {error}"))
}

fn conditional_remote_error(action: &str, error: object_store::Error) -> ConditionalObjectError {
    match error {
        object_store::Error::AlreadyExists { .. }
        | object_store::Error::Precondition { .. }
        | object_store::Error::NotFound { .. } => {
            ConditionalObjectError::Precondition(action.into())
        }
        object_store::Error::NotImplemented | object_store::Error::NotSupported { .. } => {
            ConditionalObjectError::Unsupported(action.into())
        }
        other => ConditionalObjectError::Unavailable(format!("{action}: {other}")),
    }
}

fn map_base_conditional_error(error: ImmutableObjectError) -> ConditionalObjectError {
    match error {
        ImmutableObjectError::Invalid(message) => ConditionalObjectError::Invalid(message),
        ImmutableObjectError::Conflict(message) => ConditionalObjectError::Precondition(message),
        ImmutableObjectError::Integrity(message) => ConditionalObjectError::Corrupt(message),
        ImmutableObjectError::Unavailable(message) => ConditionalObjectError::Unavailable(message),
    }
}

fn join_error(action: &'static str) -> impl FnOnce(tokio::task::JoinError) -> ImmutableObjectError {
    move |error| {
        ImmutableObjectError::Unavailable(format!(
            "local immutable object {action} failed: {error}"
        ))
    }
}
