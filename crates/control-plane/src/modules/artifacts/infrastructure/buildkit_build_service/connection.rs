use std::ffi::OsString;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone)]
pub struct BuildkitConnection {
    address: String,
    tls: Option<BuildkitMutualTls>,
}

#[derive(Debug, Clone)]
struct BuildkitMutualTls {
    server_name: String,
    ca_certificate: PathBuf,
    client_certificate: PathBuf,
    client_key: PathBuf,
}

impl BuildkitConnection {
    pub fn unix(address: impl Into<String>) -> Result<Self, String> {
        let address = address.into();
        let endpoint = parse_endpoint(&address, "unix")?;
        if endpoint.host_str().is_some() || !Path::new(endpoint.path()).is_absolute() {
            return Err("BuildKit Unix endpoint must contain an absolute socket path".into());
        }
        Ok(Self { address, tls: None })
    }

    pub fn mutual_tls(
        address: impl Into<String>,
        server_name: impl Into<String>,
        ca_certificate: impl Into<PathBuf>,
        client_certificate: impl Into<PathBuf>,
        client_key: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let address = address.into();
        let endpoint = parse_endpoint(&address, "tcp")?;
        if endpoint.host_str().is_none() || endpoint.port().is_none() {
            return Err("BuildKit mTLS endpoint must contain a host and port".into());
        }
        let server_name = server_name.into();
        if !bounded_value(&server_name, 253)
            || server_name.contains(['/', '\\', '@', ':'])
            || server_name.starts_with('.')
            || server_name.ends_with('.')
        {
            return Err("BuildKit TLS server name is invalid".into());
        }
        let ca_certificate = ca_certificate.into();
        let client_certificate = client_certificate.into();
        let client_key = client_key.into();
        for path in [&ca_certificate, &client_certificate, &client_key] {
            validate_absolute_path(path, "BuildKit mTLS credential path")?;
        }
        Ok(Self {
            address,
            tls: Some(BuildkitMutualTls {
                server_name,
                ca_certificate,
                client_certificate,
                client_key,
            }),
        })
    }

    pub fn insecure_loopback_for_conformance(address: impl Into<String>) -> Result<Self, String> {
        let address = address.into();
        let endpoint = parse_endpoint(&address, "tcp")?;
        let host = endpoint
            .host_str()
            .ok_or_else(|| "BuildKit conformance endpoint must contain a host".to_owned())?;
        let ip = host
            .parse::<IpAddr>()
            .map_err(|_| "BuildKit conformance endpoint must use a literal IP address")?;
        if !ip.is_loopback() || endpoint.port().is_none() {
            return Err("unauthenticated BuildKit is permitted only on a loopback endpoint".into());
        }
        Ok(Self { address, tls: None })
    }

    pub(super) fn arguments(&self) -> Vec<OsString> {
        let mut arguments = vec!["--addr".into(), self.address.clone().into()];
        if let Some(tls) = &self.tls {
            arguments.extend([
                "--tlsservername".into(),
                tls.server_name.clone().into(),
                "--tlscacert".into(),
                tls.ca_certificate.as_os_str().to_owned(),
                "--tlscert".into(),
                tls.client_certificate.as_os_str().to_owned(),
                "--tlskey".into(),
                tls.client_key.as_os_str().to_owned(),
            ]);
        }
        arguments
    }
}

fn parse_endpoint(address: &str, expected_scheme: &str) -> Result<Url, String> {
    if !bounded_value(address, 2048) {
        return Err("BuildKit endpoint is invalid".into());
    }
    let endpoint = Url::parse(address).map_err(|_| "BuildKit endpoint is invalid")?;
    if endpoint.scheme() != expected_scheme
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || (!endpoint.path().is_empty() && endpoint.path() != "/" && expected_scheme == "tcp")
    {
        return Err(format!(
            "BuildKit endpoint must use an exact {expected_scheme} address"
        ));
    }
    Ok(endpoint)
}

fn validate_absolute_path(path: &Path, label: &str) -> Result<(), String> {
    let value = path
        .to_str()
        .ok_or_else(|| format!("{label} must be UTF-8"))?;
    if !path.is_absolute() || !bounded_value(value, 4096) {
        return Err(format!("{label} must be a bounded absolute path"));
    }
    Ok(())
}

fn bounded_value(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}
