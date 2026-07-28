use super::{
    remote_error, remote_path, Backend, ImmutableObjectClient, ImmutableObjectError,
    ImmutableObjectWrite,
};
use futures_util::StreamExt;
use object_store::path::Path as ObjectPath;
use object_store::{MultipartUpload, ObjectStore};
use sha2::{Digest, Sha256};
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::StreamReader;
use uuid::Uuid;

const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const MULTIPART_PART_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Copy)]
struct StreamIdentity<'a> {
    expected_size: u64,
    expected_digest: &'a str,
    maximum_bytes: u64,
}

pub(crate) type ImmutableObjectReader = Pin<Box<dyn AsyncRead + Send + Unpin + 'static>>;

pub(crate) struct ImmutableObjectOpen {
    pub(crate) size_bytes: u64,
    pub(crate) reader: ImmutableObjectReader,
}

pub(crate) enum ImmutableObjectOpenResult {
    Found(ImmutableObjectOpen),
    Missing,
    Corrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImmutableObjectVerification {
    Verified,
    Missing,
    Corrupt,
}

impl ImmutableObjectClient {
    pub(crate) async fn put_stream(
        &self,
        object_key: &str,
        reader: ImmutableObjectReader,
        expected_size: u64,
        expected_digest: &str,
        maximum_bytes: u64,
    ) -> Result<ImmutableObjectWrite, ImmutableObjectError> {
        validate_stream_identity(expected_size, expected_digest, maximum_bytes)?;
        let identity = StreamIdentity {
            expected_size,
            expected_digest,
            maximum_bytes,
        };
        let scoped_key = self.scoped_key(object_key)?;
        match self.backend.as_ref() {
            Backend::Local(backend) => {
                backend
                    .put_stream(
                        scoped_key,
                        object_key.to_owned(),
                        reader,
                        expected_size,
                        expected_digest.to_owned(),
                        maximum_bytes,
                    )
                    .await
            }
            Backend::Remote(objects) => {
                self.put_remote_stream(objects, object_key, scoped_key, reader, identity)
                    .await
            }
        }
    }

    pub(crate) async fn open(
        &self,
        object_key: &str,
        maximum_bytes: u64,
    ) -> Result<ImmutableObjectOpenResult, ImmutableObjectError> {
        if maximum_bytes == 0 {
            return Err(ImmutableObjectError::Invalid(
                "immutable object stream bound must be positive".into(),
            ));
        }
        let scoped_key = self.scoped_key(object_key)?;
        match self.backend.as_ref() {
            Backend::Local(backend) => backend.open(scoped_key, maximum_bytes).await,
            Backend::Remote(objects) => {
                open_remote(objects, remote_path(&scoped_key)?, maximum_bytes).await
            }
        }
    }

    pub(crate) async fn verify(
        &self,
        object_key: &str,
        expected_size: u64,
        expected_digest: &str,
        maximum_bytes: u64,
    ) -> Result<ImmutableObjectVerification, ImmutableObjectError> {
        validate_stream_identity(expected_size, expected_digest, maximum_bytes)?;
        let mut opened = match self.open(object_key, maximum_bytes).await? {
            ImmutableObjectOpenResult::Found(opened) => opened,
            ImmutableObjectOpenResult::Missing => return Ok(ImmutableObjectVerification::Missing),
            ImmutableObjectOpenResult::Corrupt => return Ok(ImmutableObjectVerification::Corrupt),
        };
        if opened.size_bytes != expected_size {
            return Ok(ImmutableObjectVerification::Corrupt);
        }
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
        loop {
            let read = opened.reader.read(&mut buffer).await.map_err(|error| {
                ImmutableObjectError::Unavailable(format!(
                    "could not verify immutable object stream: {error}"
                ))
            })?;
            if read == 0 {
                break;
            }
            size = size.checked_add(read as u64).ok_or_else(|| {
                ImmutableObjectError::Integrity("immutable object size overflowed".into())
            })?;
            if size > expected_size || size > maximum_bytes {
                return Ok(ImmutableObjectVerification::Corrupt);
            }
            digest.update(&buffer[..read]);
        }
        if size == expected_size && format!("sha256:{:x}", digest.finalize()) == expected_digest {
            Ok(ImmutableObjectVerification::Verified)
        } else {
            Ok(ImmutableObjectVerification::Corrupt)
        }
    }

    async fn put_remote_stream(
        &self,
        objects: &Arc<dyn ObjectStore>,
        object_key: &str,
        scoped_key: String,
        mut reader: ImmutableObjectReader,
        identity: StreamIdentity<'_>,
    ) -> Result<ImmutableObjectWrite, ImmutableObjectError> {
        let final_path = remote_path(&scoped_key)?;
        let staging_path = remote_path(&format!(
            "{}/.immutable-object-staging/{}",
            self.namespace,
            Uuid::now_v7()
        ))?;
        upload_verified(
            objects,
            &staging_path,
            &mut reader,
            identity.expected_size,
            identity.expected_digest,
            identity.maximum_bytes,
        )
        .await?;

        let publication = objects.copy_if_not_exists(&staging_path, &final_path).await;
        let cleanup = objects.delete(&staging_path).await;
        if let Err(error) = cleanup {
            return Err(remote_error(
                "remove immutable object staging upload",
                error,
            ));
        }
        match publication {
            Ok(()) => Ok(ImmutableObjectWrite { created: true }),
            Err(object_store::Error::AlreadyExists { .. }) => {
                match self
                    .verify(
                        object_key,
                        identity.expected_size,
                        identity.expected_digest,
                        identity.maximum_bytes,
                    )
                    .await?
                {
                    ImmutableObjectVerification::Verified => {
                        Ok(ImmutableObjectWrite { created: false })
                    }
                    ImmutableObjectVerification::Missing | ImmutableObjectVerification::Corrupt => {
                        Err(ImmutableObjectError::Integrity(format!(
                            "stored immutable object {object_key} does not match its identity"
                        )))
                    }
                }
            }
            Err(error) => Err(remote_error("publish immutable object stream", error)),
        }
    }
}

async fn upload_verified(
    objects: &Arc<dyn ObjectStore>,
    path: &ObjectPath,
    reader: &mut ImmutableObjectReader,
    expected_size: u64,
    expected_digest: &str,
    maximum_bytes: u64,
) -> Result<(), ImmutableObjectError> {
    let mut upload = objects
        .put_multipart(path)
        .await
        .map_err(|error| remote_error("start immutable object upload", error))?;
    let result = upload_parts(
        &mut upload,
        reader,
        expected_size,
        expected_digest,
        maximum_bytes,
    )
    .await;
    if let Err(error) = result {
        return Err(abort_after_error(&mut upload, error).await);
    }
    match upload.complete().await {
        Ok(_) => Ok(()),
        Err(error) => {
            let primary = remote_error("complete immutable object upload", error);
            Err(abort_after_error(&mut upload, primary).await)
        }
    }
}

async fn upload_parts(
    upload: &mut Box<dyn MultipartUpload>,
    reader: &mut ImmutableObjectReader,
    expected_size: u64,
    expected_digest: &str,
    maximum_bytes: u64,
) -> Result<(), ImmutableObjectError> {
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut read_buffer = vec![0_u8; STREAM_BUFFER_BYTES];
    let mut part = Vec::with_capacity(MULTIPART_PART_BYTES);
    loop {
        let remaining = MULTIPART_PART_BYTES - part.len();
        let read = reader
            .read(&mut read_buffer[..remaining.min(STREAM_BUFFER_BYTES)])
            .await
            .map_err(|error| {
                ImmutableObjectError::Unavailable(format!(
                    "could not read immutable object upload: {error}"
                ))
            })?;
        if read == 0 {
            break;
        }
        size = size.checked_add(read as u64).ok_or_else(|| {
            ImmutableObjectError::Invalid("immutable object upload size overflowed".into())
        })?;
        if size > expected_size || size > maximum_bytes {
            return Err(ImmutableObjectError::Invalid(
                "immutable object upload exceeds its declared or configured size".into(),
            ));
        }
        digest.update(&read_buffer[..read]);
        part.extend_from_slice(&read_buffer[..read]);
        if part.len() == MULTIPART_PART_BYTES {
            upload
                .put_part(std::mem::take(&mut part).into())
                .await
                .map_err(|error| remote_error("write immutable object upload part", error))?;
            part = Vec::with_capacity(MULTIPART_PART_BYTES);
        }
    }
    validate_observed_identity(size, digest, expected_size, expected_digest)?;
    if !part.is_empty() {
        upload
            .put_part(part.into())
            .await
            .map_err(|error| remote_error("write immutable object upload part", error))?;
    }
    Ok(())
}

async fn abort_after_error(
    upload: &mut Box<dyn MultipartUpload>,
    primary: ImmutableObjectError,
) -> ImmutableObjectError {
    match upload.abort().await {
        Ok(()) => primary,
        Err(error) => ImmutableObjectError::Unavailable(format!(
            "{primary}; immutable object upload cleanup also failed: {error}"
        )),
    }
}

async fn open_remote(
    objects: &Arc<dyn ObjectStore>,
    path: ObjectPath,
    maximum_bytes: u64,
) -> Result<ImmutableObjectOpenResult, ImmutableObjectError> {
    let result = match objects.get(&path).await {
        Ok(result) => result,
        Err(object_store::Error::NotFound { .. }) => return Ok(ImmutableObjectOpenResult::Missing),
        Err(error) => return Err(remote_error("open immutable object stream", error)),
    };
    if result.meta.size > maximum_bytes {
        return Ok(ImmutableObjectOpenResult::Corrupt);
    }
    let size_bytes = result.meta.size;
    let stream = result.into_stream().map(|result| {
        result.map_err(|error| io::Error::other(format!("immutable object stream failed: {error}")))
    });
    Ok(ImmutableObjectOpenResult::Found(ImmutableObjectOpen {
        size_bytes,
        reader: Box::pin(StreamReader::new(stream)),
    }))
}

pub(super) fn validate_stream_identity(
    expected_size: u64,
    expected_digest: &str,
    maximum_bytes: u64,
) -> Result<(), ImmutableObjectError> {
    if expected_size == 0 || maximum_bytes == 0 || expected_size > maximum_bytes {
        return Err(ImmutableObjectError::Invalid(
            "immutable object stream size is invalid".into(),
        ));
    }
    if !expected_digest.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(ImmutableObjectError::Invalid(
            "immutable object digest must be canonical sha256".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_observed_identity(
    size: u64,
    digest: Sha256,
    expected_size: u64,
    expected_digest: &str,
) -> Result<(), ImmutableObjectError> {
    if size != expected_size {
        return Err(ImmutableObjectError::Integrity(
            "immutable object size does not match its declaration".into(),
        ));
    }
    if format!("sha256:{:x}", digest.finalize()) != expected_digest {
        return Err(ImmutableObjectError::Integrity(
            "immutable object digest does not match its declaration".into(),
        ));
    }
    Ok(())
}
