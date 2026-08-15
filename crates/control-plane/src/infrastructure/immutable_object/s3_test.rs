use super::{ImmutableObjectClient, S3ImmutableObjectOptions};
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
pub(crate) struct DisposableS3TestContext {
    client: ImmutableObjectClient,
    secure_transport: bool,
}

impl DisposableS3TestContext {
    pub(crate) fn from_environment(suite: &str) -> Result<Self, String> {
        validate_suite(suite)?;
        let endpoint = required_environment(ENDPOINT_ENV)?;
        let endpoint_url = Url::parse(&endpoint)
            .map_err(|_| format!("{ENDPOINT_ENV} must be an absolute HTTP(S) URL"))?;
        if !matches!(endpoint_url.scheme(), "http" | "https")
            || endpoint_url.host_str().is_none()
            || !endpoint_url.username().is_empty()
            || endpoint_url.password().is_some()
            || endpoint_url.query().is_some()
            || endpoint_url.fragment().is_some()
        {
            return Err(format!(
                "{ENDPOINT_ENV} must be a credential-free HTTP(S) origin"
            ));
        }
        let region = std::env::var(REGION_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "us-east-1".into());
        let bucket = required_environment(BUCKET_ENV)?;
        let access_key_id = required_environment(ACCESS_KEY_ENV)?;
        let secret_access_key = required_environment(SECRET_KEY_ENV)?;
        let session_token = optional_environment(SESSION_TOKEN_ENV)?;
        let virtual_hosted_style = optional_boolean(VIRTUAL_HOSTED_STYLE_ENV)?;
        let prefix = format!("a3s-cloud-tests/{suite}/{}", Uuid::now_v7());
        let secure_transport = endpoint_url.scheme() == "https";
        let client = ImmutableObjectClient::s3(S3ImmutableObjectOptions {
            endpoint: Some(endpoint),
            region,
            bucket,
            prefix,
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
        })
    }

    pub(crate) fn client(&self) -> ImmutableObjectClient {
        self.client.clone()
    }

    pub(crate) fn uses_secure_transport(&self) -> bool {
        self.secure_transport
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
