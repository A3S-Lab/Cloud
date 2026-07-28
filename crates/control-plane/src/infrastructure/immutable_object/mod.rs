mod local;
mod s3;

#[cfg(test)]
mod tests;

use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, PutMode};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) use s3::S3ImmutableObjectOptions;

const MAX_OBJECT_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImmutableObjectWrite {
    pub(crate) created: bool,
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

fn remote_path(scoped_key: &str) -> Result<ObjectPath, ImmutableObjectError> {
    ObjectPath::parse(scoped_key).map_err(|error| ImmutableObjectError::Invalid(error.to_string()))
}

fn remote_error(action: &str, error: object_store::Error) -> ImmutableObjectError {
    ImmutableObjectError::Unavailable(format!("{action}: {error}"))
}

fn join_error(action: &'static str) -> impl FnOnce(tokio::task::JoinError) -> ImmutableObjectError {
    move |error| {
        ImmutableObjectError::Unavailable(format!(
            "local immutable object {action} failed: {error}"
        ))
    }
}
