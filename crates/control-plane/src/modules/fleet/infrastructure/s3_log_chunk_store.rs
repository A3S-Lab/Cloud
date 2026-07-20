use super::log_chunk_object::{
    prepare_log_object, validate_expected_checksum, validate_object_key, verify_log_object,
    MAX_LOG_OBJECT_BYTES,
};
use crate::modules::fleet::domain::services::{
    ILogChunkStore, LogChunkStoreError, RetrievedLogChunk, StoredLogChunk,
};
use a3s_cloud_contracts::NodeLogChunkReport;
use async_trait::async_trait;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::{ClientOptions, ObjectStore, PutMode, RetryConfig};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub(crate) struct S3LogChunkStoreOptions {
    pub(crate) endpoint: Option<String>,
    pub(crate) region: String,
    pub(crate) bucket: String,
    pub(crate) prefix: String,
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) session_token: Option<String>,
    pub(crate) allow_http: bool,
    pub(crate) virtual_hosted_style: bool,
    pub(crate) request_timeout: Duration,
    pub(crate) connect_timeout: Duration,
    pub(crate) retry_timeout: Duration,
    pub(crate) max_retries: usize,
}

impl fmt::Debug for S3LogChunkStoreOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3LogChunkStoreOptions")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("access_key_id", &"<redacted>")
            .field("secret_access_key", &"<redacted>")
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .field("allow_http", &self.allow_http)
            .field("virtual_hosted_style", &self.virtual_hosted_style)
            .field("request_timeout", &self.request_timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("retry_timeout", &self.retry_timeout)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct S3LogChunkStore {
    objects: Arc<dyn ObjectStore>,
    prefix: ObjectPath,
}

impl fmt::Debug for S3LogChunkStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3LogChunkStore")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl S3LogChunkStore {
    pub(crate) fn new(options: S3LogChunkStoreOptions) -> Result<Self, LogChunkStoreError> {
        validate_options(&options)?;
        let prefix = ObjectPath::parse(&options.prefix)
            .map_err(|error| LogChunkStoreError::Invalid(error.to_string()))?;
        let client_options = ClientOptions::new()
            .with_allow_http(options.allow_http)
            .with_timeout(options.request_timeout)
            .with_connect_timeout(options.connect_timeout);
        let retry = RetryConfig {
            max_retries: options.max_retries,
            retry_timeout: options.retry_timeout,
            ..RetryConfig::default()
        };
        let mut builder = AmazonS3Builder::new()
            .with_region(options.region)
            .with_bucket_name(options.bucket)
            .with_access_key_id(options.access_key_id)
            .with_secret_access_key(options.secret_access_key)
            .with_virtual_hosted_style_request(options.virtual_hosted_style)
            .with_client_options(client_options)
            .with_retry(retry);
        if let Some(endpoint) = options.endpoint {
            builder = builder.with_endpoint(endpoint);
        }
        if let Some(session_token) = options.session_token {
            builder = builder.with_token(session_token);
        }
        let objects = builder
            .build()
            .map_err(|error| LogChunkStoreError::Invalid(error.to_string()))?;
        Ok(Self {
            objects: Arc::new(objects),
            prefix,
        })
    }

    #[cfg(test)]
    fn from_store(objects: Arc<dyn ObjectStore>, prefix: &str) -> Result<Self, LogChunkStoreError> {
        let prefix = ObjectPath::parse(prefix)
            .map_err(|error| LogChunkStoreError::Invalid(error.to_string()))?;
        Ok(Self { objects, prefix })
    }

    fn object_path(&self, object_key: &str) -> Result<ObjectPath, LogChunkStoreError> {
        validate_object_key(object_key)?;
        ObjectPath::parse(format!("{}/{object_key}", self.prefix))
            .map_err(|error| LogChunkStoreError::Invalid(error.to_string()))
    }

    async fn existing_matches(
        &self,
        path: &ObjectPath,
        expected: &[u8],
    ) -> Result<bool, LogChunkStoreError> {
        let result = self
            .objects
            .get(path)
            .await
            .map_err(|error| unavailable("read existing S3 log object", error))?;
        if result.meta.size > MAX_LOG_OBJECT_BYTES {
            return Ok(false);
        }
        let body = result
            .bytes()
            .await
            .map_err(|error| unavailable("collect existing S3 log object", error))?;
        Ok(body.as_ref() == expected)
    }

    async fn delete_path(&self, path: &ObjectPath) -> Result<(), LogChunkStoreError> {
        match self.objects.delete(path).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(unavailable("delete S3 log object", error)),
        }
    }
}

#[async_trait]
impl ILogChunkStore for S3LogChunkStore {
    async fn put(
        &self,
        _batch_id: Uuid,
        node_id: Uuid,
        _ordinal: u16,
        report: &NodeLogChunkReport,
    ) -> Result<StoredLogChunk, LogChunkStoreError> {
        let (object_key, body) = prepare_log_object(node_id, report)?;
        let path = self.object_path(&object_key)?;
        let created = match self
            .objects
            .put_opts(&path, body.clone().into(), PutMode::Create.into())
            .await
        {
            Ok(_) => true,
            Err(object_store::Error::AlreadyExists { .. }) => {
                if self.existing_matches(&path, &body).await? {
                    false
                } else {
                    return Err(LogChunkStoreError::Conflict(object_key));
                }
            }
            Err(error) => return Err(unavailable("write immutable S3 log object", error)),
        };
        Ok(StoredLogChunk {
            object_key,
            created,
        })
    }

    async fn get(
        &self,
        object_key: &str,
        expected_checksum: &str,
    ) -> Result<RetrievedLogChunk, LogChunkStoreError> {
        validate_expected_checksum(expected_checksum)?;
        let path = self.object_path(object_key)?;
        let result = match self.objects.get(&path).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(RetrievedLogChunk::Missing),
            Err(error) => return Err(unavailable("read S3 log object", error)),
        };
        if result.meta.size > MAX_LOG_OBJECT_BYTES {
            return Ok(RetrievedLogChunk::Corrupt);
        }
        let body = result
            .bytes()
            .await
            .map_err(|error| unavailable("collect S3 log object", error))?;
        verify_log_object(&body, expected_checksum)
    }

    async fn remove(&self, object_key: &str) -> Result<(), LogChunkStoreError> {
        let path = self.object_path(object_key)?;
        self.delete_path(&path).await
    }

    async fn health(&self) -> Result<bool, LogChunkStoreError> {
        let path = ObjectPath::parse(format!("{}/.health/{}", self.prefix, Uuid::now_v7()))
            .map_err(|error| LogChunkStoreError::Invalid(error.to_string()))?;
        self.objects
            .put_opts(&path, b"ok".as_slice().into(), PutMode::Create.into())
            .await
            .map_err(|error| unavailable("write S3 log store health probe", error))?;
        let read = async {
            let body = self
                .objects
                .get(&path)
                .await
                .map_err(|error| unavailable("read S3 log store health probe", error))?
                .bytes()
                .await
                .map_err(|error| unavailable("collect S3 log store health probe", error))?;
            if body.as_ref() != b"ok" {
                return Err(LogChunkStoreError::Unavailable(
                    "S3 log store health probe changed after write".into(),
                ));
            }
            Ok(())
        }
        .await;
        let removed = self.delete_path(&path).await;
        match (read, removed) {
            (Ok(()), Ok(())) => Ok(true),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(read_error), Err(delete_error)) => Err(LogChunkStoreError::Unavailable(format!(
                "{read_error}; cleanup also failed: {delete_error}"
            ))),
        }
    }
}

fn validate_options(options: &S3LogChunkStoreOptions) -> Result<(), LogChunkStoreError> {
    if options.region.is_empty()
        || options.bucket.is_empty()
        || options.prefix.is_empty()
        || options.access_key_id.is_empty()
        || options.secret_access_key.is_empty()
        || options.request_timeout.is_zero()
        || options.connect_timeout.is_zero()
        || options.connect_timeout > options.request_timeout
        || options.retry_timeout < options.request_timeout
        || options.max_retries > 10
    {
        return Err(LogChunkStoreError::Invalid(
            "S3 log store options are invalid".into(),
        ));
    }
    if options.session_token.as_deref().is_some_and(str::is_empty) {
        return Err(LogChunkStoreError::Invalid(
            "S3 session token must be absent or nonempty".into(),
        ));
    }
    Ok(())
}

fn unavailable(action: &str, error: object_store::Error) -> LogChunkStoreError {
    LogChunkStoreError::Unavailable(format!("{action}: {error}"))
}

#[cfg(test)]
#[path = "s3_log_chunk_store_tests.rs"]
mod tests;
