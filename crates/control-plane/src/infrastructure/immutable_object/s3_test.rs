use super::{Backend, ImmutableObjectClient, S3ImmutableObjectOptions};
use futures_util::TryStreamExt;
use object_store::path::Path as ObjectPath;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

const ENDPOINT_ENV: &str = "A3S_CLOUD_TEST_S3_ENDPOINT";
const REGION_ENV: &str = "A3S_CLOUD_TEST_S3_REGION";
const BUCKET_ENV: &str = "A3S_CLOUD_TEST_S3_BUCKET";
const ACCESS_KEY_ENV: &str = "A3S_CLOUD_TEST_S3_ACCESS_KEY_ID";
const SECRET_KEY_ENV: &str = "A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY";
const SESSION_TOKEN_ENV: &str = "A3S_CLOUD_TEST_S3_SESSION_TOKEN";
const VIRTUAL_HOSTED_STYLE_ENV: &str = "A3S_CLOUD_TEST_S3_VIRTUAL_HOSTED_STYLE";

/// One shared test-only constructor for disposable real S3-compatible gates.
///
/// It deliberately returns the production `ImmutableObjectClient`; consumers
/// cannot obtain the raw provider client or introduce another S3 builder.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct DisposableS3TestContext {
    client: ImmutableObjectClient,
    secure_transport: bool,
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    virtual_hosted_style: bool,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl DisposableS3TestContext {
    pub(crate) fn from_environment(suite: &str) -> Result<Self, String> {
        Self::from_environment_with_id(suite, Uuid::now_v7())
    }

    pub(crate) fn from_environment_with_id(suite: &str, id: Uuid) -> Result<Self, String> {
        validate_suite(suite)?;
        if id.is_nil() {
            return Err("disposable S3 test namespace ID must not be nil".into());
        }
        let configured_endpoint = required_environment(ENDPOINT_ENV)?;
        let endpoint_url = Url::parse(&configured_endpoint)
            .map_err(|_| format!("{ENDPOINT_ENV} must be an absolute HTTP(S) URL"))?;
        if !matches!(endpoint_url.scheme(), "http" | "https")
            || endpoint_url.host_str().is_none()
            || endpoint_url.path() != "/"
            || !endpoint_url.username().is_empty()
            || endpoint_url.password().is_some()
            || endpoint_url.query().is_some()
            || endpoint_url.fragment().is_some()
        {
            return Err(format!(
                "{ENDPOINT_ENV} must be a credential-free HTTP(S) origin"
            ));
        }
        let endpoint = endpoint_url.origin().ascii_serialization();
        let region = std::env::var(REGION_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "us-east-1".into());
        let bucket = required_environment(BUCKET_ENV)?;
        let access_key_id = required_environment(ACCESS_KEY_ENV)?;
        let secret_access_key = required_environment(SECRET_KEY_ENV)?;
        let session_token = optional_environment(SESSION_TOKEN_ENV)?;
        let virtual_hosted_style = optional_boolean(VIRTUAL_HOSTED_STYLE_ENV)?;
        let prefix = format!("a3s-cloud-tests/{suite}/{id}");
        let secure_transport = endpoint_url.scheme() == "https";
        let client = ImmutableObjectClient::s3(S3ImmutableObjectOptions {
            endpoint: Some(endpoint.clone()),
            region: region.clone(),
            bucket: bucket.clone(),
            prefix: prefix.clone(),
            access_key_id,
            secret_access_key,
            session_token,
            allow_http: !secure_transport,
            virtual_hosted_style,
            request_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            retry_timeout: Duration::from_secs(60),
            max_retries: 3,
        })
        .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            secure_transport,
            endpoint,
            region,
            bucket,
            prefix,
            virtual_hosted_style,
        })
    }

    pub(crate) fn client(&self) -> ImmutableObjectClient {
        self.client.clone()
    }

    pub(crate) fn uses_secure_transport(&self) -> bool {
        self.secure_transport
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) fn region(&self) -> &str {
        &self.region
    }

    pub(crate) fn bucket(&self) -> &str {
        &self.bucket
    }

    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    pub(crate) fn virtual_hosted_style(&self) -> bool {
        self.virtual_hosted_style
    }

    /// Removes and then re-lists the unique disposable namespace. This is
    /// deliberately test-only and reaches the same production object-store
    /// handle used by `IObjectNamespace`; it does not construct another S3
    /// client or create a product-level list/delete lifecycle.
    pub(crate) async fn remove_all(&self) -> Result<usize, String> {
        let Backend::Remote(objects) = self.client.backend.as_ref() else {
            return Err("disposable S3 cleanup requires the remote backend".into());
        };
        let prefix = ObjectPath::from(self.prefix.clone());
        let locations = objects
            .list(Some(&prefix))
            .map_ok(|metadata| metadata.location)
            .try_collect::<Vec<_>>()
            .await
            .map_err(|error| format!("list disposable S3 namespace for cleanup: {error}"))?;
        for location in &locations {
            if !location.as_ref().starts_with(&format!("{}/", self.prefix)) {
                return Err("disposable S3 cleanup escaped its exact namespace".into());
            }
            objects
                .delete(location)
                .await
                .map_err(|error| format!("delete disposable S3 object: {error}"))?;
        }
        let remaining = objects
            .list(Some(&prefix))
            .try_collect::<Vec<_>>()
            .await
            .map_err(|error| format!("verify disposable S3 namespace cleanup: {error}"))?;
        if !remaining.is_empty() {
            return Err("disposable S3 namespace cleanup retained objects".into());
        }
        Ok(locations.len())
    }
}

fn validate_suite(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    {
        return Err("disposable S3 test suite name is invalid".into());
    }
    Ok(())
}

fn required_environment(name: &str) -> Result<String, String> {
    let value = std::env::var(name).map_err(|_| format!("{name} is required"))?;
    if value.is_empty() || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{name} is invalid"));
    }
    Ok(value)
}

fn optional_environment(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) if value.contains(['\0', '\r', '\n']) => Err(format!("{name} is invalid")),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} is invalid")),
    }
}

fn optional_boolean(name: &str) -> Result<bool, String> {
    match std::env::var(name) {
        Ok(value) if value == "true" => Ok(true),
        Ok(value) if value == "false" => Ok(false),
        Ok(_) => Err(format!("{name} must be true or false")),
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} is invalid")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_names_are_bounded_safe_path_segments() {
        validate_suite("s0-cas").expect("suite");
        assert!(validate_suite("").is_err());
        assert!(validate_suite("S0").is_err());
        assert!(validate_suite("../s0").is_err());
        assert!(validate_suite(&"a".repeat(65)).is_err());
    }
}
