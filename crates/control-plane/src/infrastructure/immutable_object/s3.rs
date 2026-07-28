use super::ImmutableObjectError;
use object_store::aws::{AmazonS3Builder, S3CopyIfNotExists};
use object_store::{ClientOptions, ObjectStore, RetryConfig};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct S3ImmutableObjectOptions {
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

impl fmt::Debug for S3ImmutableObjectOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3ImmutableObjectOptions")
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

pub(super) fn build(
    options: S3ImmutableObjectOptions,
) -> Result<Arc<dyn ObjectStore>, ImmutableObjectError> {
    validate(&options)?;
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
        .with_copy_if_not_exists(S3CopyIfNotExists::Multipart)
        .with_client_options(client_options)
        .with_retry(retry);
    if let Some(endpoint) = options.endpoint {
        builder = builder.with_endpoint(endpoint);
    }
    if let Some(session_token) = options.session_token {
        builder = builder.with_token(session_token);
    }
    builder
        .build()
        .map(|objects| Arc::new(objects) as Arc<dyn ObjectStore>)
        .map_err(|error| ImmutableObjectError::Invalid(error.to_string()))
}

fn validate(options: &S3ImmutableObjectOptions) -> Result<(), ImmutableObjectError> {
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
        return Err(ImmutableObjectError::Invalid(
            "S3 immutable object options are invalid".into(),
        ));
    }
    if options.session_token.as_deref().is_some_and(str::is_empty) {
        return Err(ImmutableObjectError::Invalid(
            "S3 session token must be absent or nonempty".into(),
        ));
    }
    Ok(())
}
