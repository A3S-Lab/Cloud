use a3s_acl::{Block, Document, Value};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessRole {
    All,
    Api,
    Worker,
    Relay,
}

impl ProcessRole {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "all" => Ok(Self::All),
            "api" => Ok(Self::Api),
            "worker" => Ok(Self::Worker),
            "relay" => Ok(Self::Relay),
            _ => Err(ConfigError::Invalid(format!(
                "server.role {value:?} must be all, api, worker, or relay"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub role: ProcessRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresConfig {
    pub url_env: String,
    pub max_connections: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub bootstrap_token_env: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventProviderKind {
    Memory,
    Nats,
}

impl EventProviderKind {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "memory" => Ok(Self::Memory),
            "nats" => Ok(Self::Nats),
            _ => Err(ConfigError::Invalid(format!(
                "events.provider {value:?} must be memory or nats"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventsConfig {
    pub provider: EventProviderKind,
    pub nats_url_env: String,
    pub stream_name: String,
    pub batch_size: usize,
    pub poll_interval_ms: u64,
    pub lease_ms: u64,
    pub publish_timeout_ms: u64,
    pub retry_initial_ms: u64,
    pub retry_max_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationsConfig {
    pub reconcile_interval_ms: u64,
    pub lease_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudConfig {
    pub server: ServerConfig,
    pub postgres: PostgresConfig,
    pub auth: AuthConfig,
    pub events: EventsConfig,
    pub operations: OperationsConfig,
}

impl CloudConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        let document = a3s_acl::parse(source)
            .map_err(|error| ConfigError::Invalid(format!("invalid HCL: {error}")))?;
        validate_root(&document)?;
        let server = one_block(&document, "server")?;
        validate_block(server, &["host", "port", "role"])?;
        let postgres = one_block(&document, "postgres")?;
        validate_block(postgres, &["url_env", "max_connections"])?;
        let auth = one_block(&document, "auth")?;
        validate_block(auth, &["bootstrap_token_env"])?;
        let events = one_block(&document, "events")?;
        validate_block(
            events,
            &[
                "provider",
                "nats_url_env",
                "stream_name",
                "batch_size",
                "poll_interval_ms",
                "lease_ms",
                "publish_timeout_ms",
                "retry_initial_ms",
                "retry_max_ms",
            ],
        )?;
        let operations = one_block(&document, "operations")?;
        validate_block(operations, &["reconcile_interval_ms", "lease_ms"])?;

        let config = Self {
            server: ServerConfig {
                host: string(server, "host")?,
                port: integer(server, "port")?,
                role: ProcessRole::parse(&string(server, "role")?)?,
            },
            postgres: PostgresConfig {
                url_env: string(postgres, "url_env")?,
                max_connections: integer(postgres, "max_connections")?,
            },
            auth: AuthConfig {
                bootstrap_token_env: string(auth, "bootstrap_token_env")?,
            },
            events: EventsConfig {
                provider: EventProviderKind::parse(&string(events, "provider")?)?,
                nats_url_env: string(events, "nats_url_env")?,
                stream_name: string(events, "stream_name")?,
                batch_size: integer(events, "batch_size")?,
                poll_interval_ms: integer(events, "poll_interval_ms")?,
                lease_ms: integer(events, "lease_ms")?,
                publish_timeout_ms: integer(events, "publish_timeout_ms")?,
                retry_initial_ms: integer(events, "retry_initial_ms")?,
                retry_max_ms: integer(events, "retry_max_ms")?,
            },
            operations: OperationsConfig {
                reconcile_interval_ms: integer(operations, "reconcile_interval_ms")?,
                lease_ms: integer(operations, "lease_ms")?,
            },
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.server.host.trim().is_empty() || self.server.host.len() > 255 {
            return Err(ConfigError::Invalid(
                "server.host must be a bounded nonempty value".into(),
            ));
        }
        if self.server.port == 0 {
            return Err(ConfigError::Invalid(
                "server.port must be greater than zero".into(),
            ));
        }
        if !valid_env_name(&self.postgres.url_env) {
            return Err(ConfigError::Invalid(
                "postgres.url_env must be an uppercase environment variable name".into(),
            ));
        }
        if self.postgres.max_connections == 0 || self.postgres.max_connections > 1024 {
            return Err(ConfigError::Invalid(
                "postgres.max_connections must be between 1 and 1024".into(),
            ));
        }
        if !valid_env_name(&self.auth.bootstrap_token_env) {
            return Err(ConfigError::Invalid(
                "auth.bootstrap_token_env must be an uppercase environment variable name".into(),
            ));
        }
        if !valid_env_name(&self.events.nats_url_env) {
            return Err(ConfigError::Invalid(
                "events.nats_url_env must be an uppercase environment variable name".into(),
            ));
        }
        if self.events.stream_name.is_empty()
            || self.events.stream_name.len() > 63
            || !self
                .events
                .stream_name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(ConfigError::Invalid(
                "events.stream_name must contain 1 to 63 uppercase letters, digits, or underscores"
                    .into(),
            ));
        }
        if self.events.batch_size == 0
            || self.events.batch_size > 10_000
            || self.events.poll_interval_ms == 0
            || self.events.publish_timeout_ms == 0
            || self.events.lease_ms <= self.events.publish_timeout_ms
            || self.events.retry_initial_ms == 0
            || self.events.retry_max_ms < self.events.retry_initial_ms
        {
            return Err(ConfigError::Invalid(
                "events relay requires a batch of 1 to 10000, positive independent timings, a lease longer than publish timeout, and ordered retry bounds"
                    .into(),
            ));
        }
        if self.operations.reconcile_interval_ms == 0
            || self.operations.lease_ms <= self.operations.reconcile_interval_ms
        {
            return Err(ConfigError::Invalid(
                "operations.lease_ms must exceed a positive reconcile interval".into(),
            ));
        }
        Ok(())
    }

    pub fn server_address(&self) -> Result<SocketAddr, ConfigError> {
        format!("{}:{}", self.server.host, self.server.port)
            .parse()
            .map_err(|error| ConfigError::Invalid(format!("invalid server address: {error}")))
    }

    pub fn postgres_url(&self) -> Result<String, ConfigError> {
        std::env::var(&self.postgres.url_env).map_err(|_| {
            ConfigError::Invalid(format!(
                "required environment variable {:?} is not set",
                self.postgres.url_env
            ))
        })
    }

    pub fn nats_url(&self) -> Result<Option<String>, ConfigError> {
        if self.events.provider == EventProviderKind::Memory {
            return Ok(None);
        }
        std::env::var(&self.events.nats_url_env)
            .map(Some)
            .map_err(|_| {
                ConfigError::Invalid(format!(
                    "required environment variable {:?} is not set",
                    self.events.nats_url_env
                ))
            })
    }

    pub fn bootstrap_token(&self) -> Result<String, ConfigError> {
        let value = std::env::var(&self.auth.bootstrap_token_env).map_err(|_| {
            ConfigError::Invalid(format!(
                "required environment variable {:?} is not set",
                self.auth.bootstrap_token_env
            ))
        })?;
        if value.len() < 32 || value.len() > 512 || value.contains(['\0', '\r', '\n']) {
            return Err(ConfigError::Invalid(format!(
                "environment variable {:?} must contain 32 to 512 safe bytes",
                self.auth.bootstrap_token_env
            )));
        }
        Ok(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read Cloud config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid Cloud config: {0}")]
    Invalid(String),
}

fn validate_root(document: &Document) -> Result<(), ConfigError> {
    let allowed = ["auth", "events", "operations", "postgres", "server"];
    if document
        .blocks
        .iter()
        .any(|block| !allowed.contains(&block.name.as_str()))
    {
        return Err(ConfigError::Invalid(
            "config contains an unsupported root block".into(),
        ));
    }
    Ok(())
}

fn valid_env_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn one_block<'a>(document: &'a Document, name: &str) -> Result<&'a Block, ConfigError> {
    let blocks = document
        .blocks
        .iter()
        .filter(|block| block.name == name)
        .collect::<Vec<_>>();
    if blocks.len() != 1 {
        return Err(ConfigError::Invalid(format!(
            "config must contain exactly one {name} block"
        )));
    }
    Ok(blocks[0])
}

fn validate_block(block: &Block, fields: &[&str]) -> Result<(), ConfigError> {
    if !block.labels.is_empty() || !block.blocks.is_empty() {
        return Err(ConfigError::Invalid(format!(
            "{} block cannot contain labels or nested blocks",
            block.name
        )));
    }
    let expected = fields.iter().copied().collect::<BTreeSet<_>>();
    let actual = block
        .attributes
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(ConfigError::Invalid(format!(
            "{} block must contain exactly {}",
            block.name,
            fields.join(", ")
        )));
    }
    Ok(())
}

fn string(block: &Block, field: &str) -> Result<String, ConfigError> {
    block
        .attributes
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ConfigError::Invalid(format!("{}.{} must be a string", block.name, field)))
}

fn integer<T>(block: &Block, field: &str) -> Result<T, ConfigError>
where
    T: TryFrom<u64>,
{
    let number = block
        .attributes
        .get(field)
        .and_then(Value::as_number)
        .ok_or_else(|| {
            ConfigError::Invalid(format!("{}.{} must be an integer", block.name, field))
        })?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > u64::MAX as f64 {
        return Err(ConfigError::Invalid(format!(
            "{}.{} must be a nonnegative integer",
            block.name, field
        )));
    }
    T::try_from(number as u64)
        .map_err(|_| ConfigError::Invalid(format!("{}.{} is out of range", block.name, field)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
server { host = "127.0.0.1" port = 8080 role = "all" }
postgres { url_env = "A3S_CLOUD_POSTGRES_URL" max_connections = 16 }
auth { bootstrap_token_env = "A3S_CLOUD_BOOTSTRAP_TOKEN" }
events {
  provider = "memory"
  nats_url_env = "A3S_CLOUD_NATS_URL"
  stream_name = "A3S_CLOUD_EVENTS"
  batch_size = 100
  poll_interval_ms = 250
  lease_ms = 10000
  publish_timeout_ms = 3000
  retry_initial_ms = 500
  retry_max_ms = 30000
}
operations { reconcile_interval_ms = 5000 lease_ms = 30000 }
"#;

    #[test]
    fn parses_closed_hcl_configuration() {
        let config = CloudConfig::parse(VALID).expect("valid config");
        assert_eq!(config.server.role, ProcessRole::All);
        assert_eq!(config.server_address().expect("address").port(), 8080);
        assert_eq!(config.postgres.max_connections, 16);
        assert_eq!(config.auth.bootstrap_token_env, "A3S_CLOUD_BOOTSTRAP_TOKEN");
        assert_eq!(config.events.provider, EventProviderKind::Memory);
    }

    #[test]
    fn rejects_unknown_fields_and_unsafe_timing() {
        assert!(CloudConfig::parse(
            &VALID.replace("role = \"all\"", "role = \"all\" debug = true")
        )
        .is_err());
        assert!(CloudConfig::parse(&VALID.replace("lease_ms = 30000", "lease_ms = 1000")).is_err());
        assert!(CloudConfig::parse(
            &VALID.replace("publish_timeout_ms = 3000", "publish_timeout_ms = 10000")
        )
        .is_err());
    }
}
