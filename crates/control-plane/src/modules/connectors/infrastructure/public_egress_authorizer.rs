use crate::modules::connectors::domain::{
    validate_resolved_connector_endpoint, AuthorizedConnectorDestination, ConnectorExecutionError,
    IConnectorEgressAuthorizer, MAXIMUM_AUTHORIZED_CONNECTOR_ADDRESSES,
};
use crate::modules::shared_kernel::domain::ConnectorRevisionId;
use async_trait::async_trait;
use hickory_resolver::{error::ResolveErrorKind, TokioAsyncResolver};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use url::{Host, Url};

const MAXIMUM_DNS_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

// IANA Special-Use Domain Names, last reviewed 2026-08-15. The registry's
// designation applies to each listed name and all of its subdomains. ARPA is
// rejected as a whole because its registered children are infrastructure or
// local reverse/service namespaces, not public Connector destinations.
// https://www.iana.org/assignments/special-use-domain-names/
const SPECIAL_USE_DOMAIN_SUFFIXES: &[&str] = &[
    "alt",
    "arpa",
    "example",
    "example.com",
    "example.net",
    "example.org",
    "invalid",
    "local",
    "localhost",
    "onion",
    "test",
];

/// Production egress policy for public HTTPS Connector destinations.
///
/// The policy resolves an absolute DNS name for every attempt, rejects the
/// entire answer set when any address is not public, and returns the exact
/// bounded addresses that the HTTP executor must pin for that attempt. It does
/// not cache Connector policy, perform HTTP, or own retry scheduling.
pub struct PublicInternetConnectorEgressAuthorizer {
    resolver: Arc<dyn DnsIpResolver>,
    lookup_timeout: Duration,
}

impl PublicInternetConnectorEgressAuthorizer {
    pub fn from_system_config(lookup_timeout: Duration) -> Result<Self, String> {
        let resolver = TokioAsyncResolver::tokio_from_system_conf()
            .map_err(|_| "system DNS resolver configuration is unavailable".to_owned())?;
        Self::with_resolver(Arc::new(SystemDnsIpResolver { resolver }), lookup_timeout)
    }

    fn with_resolver(
        resolver: Arc<dyn DnsIpResolver>,
        lookup_timeout: Duration,
    ) -> Result<Self, String> {
        if lookup_timeout.is_zero() || lookup_timeout > MAXIMUM_DNS_LOOKUP_TIMEOUT {
            return Err(
                "Connector DNS lookup timeout must be between 1 millisecond and 10 seconds".into(),
            );
        }
        Ok(Self {
            resolver,
            lookup_timeout,
        })
    }

    async fn resolve_domain(&self, host: &str) -> Result<Vec<IpAddr>, ConnectorExecutionError> {
        let normalized =
            normalize_public_dns_name(host).ok_or(ConnectorExecutionError::Rejected)?;
        let absolute_name = format!("{normalized}.");
        tokio::time::timeout(self.lookup_timeout, self.resolver.lookup_ip(&absolute_name))
            .await
            .map_err(|_| retryable_dns_failure())?
            .map_err(|_| retryable_dns_failure())
    }
}

impl fmt::Debug for PublicInternetConnectorEgressAuthorizer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublicInternetConnectorEgressAuthorizer")
            .field("lookup_timeout", &self.lookup_timeout)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl IConnectorEgressAuthorizer for PublicInternetConnectorEgressAuthorizer {
    async fn authorize(
        &self,
        connector_revision_id: ConnectorRevisionId,
        endpoint: &Url,
    ) -> Result<AuthorizedConnectorDestination, ConnectorExecutionError> {
        if connector_revision_id.as_uuid().is_nil()
            || validate_resolved_connector_endpoint(endpoint, false, true).is_err()
        {
            return Err(ConnectorExecutionError::Rejected);
        }

        let addresses = match endpoint.host() {
            Some(Host::Ipv4(address)) => vec![IpAddr::V4(address)],
            Some(Host::Ipv6(address)) => vec![IpAddr::V6(address)],
            Some(Host::Domain(host)) => self.resolve_domain(host).await?,
            None => return Err(ConnectorExecutionError::Rejected),
        };
        if addresses.is_empty() {
            return Err(retryable_dns_failure());
        }
        if addresses.len() > MAXIMUM_AUTHORIZED_CONNECTOR_ADDRESSES
            || addresses.iter().any(|address| !is_public_ip(*address))
        {
            return Err(ConnectorExecutionError::Rejected);
        }

        let port = endpoint
            .port_or_known_default()
            .ok_or(ConnectorExecutionError::Rejected)?;
        AuthorizedConnectorDestination::new(
            endpoint,
            addresses
                .into_iter()
                .map(|address| SocketAddr::new(address, port))
                .collect(),
        )
    }
}

#[async_trait]
trait DnsIpResolver: Send + Sync {
    async fn lookup_ip(&self, absolute_name: &str) -> Result<Vec<IpAddr>, DnsIpResolutionError>;
}

#[derive(Debug, Clone, Copy)]
struct DnsIpResolutionError;

struct SystemDnsIpResolver {
    resolver: TokioAsyncResolver,
}

#[async_trait]
impl DnsIpResolver for SystemDnsIpResolver {
    async fn lookup_ip(&self, absolute_name: &str) -> Result<Vec<IpAddr>, DnsIpResolutionError> {
        match self.resolver.lookup_ip(absolute_name).await {
            Ok(lookup) => Ok(lookup
                .iter()
                .take(MAXIMUM_AUTHORIZED_CONNECTOR_ADDRESSES + 1)
                .collect()),
            Err(error) if matches!(error.kind(), ResolveErrorKind::NoRecordsFound { .. }) => {
                Ok(Vec::new())
            }
            Err(_) => Err(DnsIpResolutionError),
        }
    }
}

fn normalize_public_dns_name(host: &str) -> Option<String> {
    let normalized = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    let labels = normalized.split('.').collect::<Vec<_>>();
    let valid_labels = labels.len() >= 2
        && normalized.len() <= 253
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        });
    if !valid_labels
        || SPECIAL_USE_DOMAIN_SUFFIXES
            .iter()
            .any(|suffix| normalized == *suffix || normalized.ends_with(&format!(".{suffix}")))
    {
        return None;
    }
    Some(normalized)
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

// Explicitly follows the IANA IPv4 Special-Purpose Address Space registry
// reviewed 2026-08-15, plus multicast/reserved space. The two globally
// reachable anycast exceptions in 192.0.0.0/24 remain allowed.
// https://www.iana.org/assignments/iana-ipv4-special-registry/
fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, third, fourth] = address.octets();
    if first == 192 && second == 0 && third == 0 {
        return matches!(fourth, 9 | 10);
    }
    !(first == 0
        || first == 10
        || (first == 100 && (64..=127).contains(&second))
        || first == 127
        || (first == 169 && second == 254)
        || (first == 172 && (16..=31).contains(&second))
        || (first == 192 && second == 0 && third == 2)
        || (first == 192 && second == 88 && third == 99)
        || (first == 192 && second == 168)
        || (first == 198 && matches!(second, 18 | 19))
        || (first == 198 && second == 51 && third == 100)
        || (first == 203 && second == 0 && third == 113)
        || first >= 224)
}

// IANA currently allocates ordinary IPv6 global unicast from 2000::/3. This
// public-Internet policy conservatively rejects special protocol, transition,
// and documentation blocks even where a narrower exception may be globally
// reachable; Connectors do not need those protocol-specific destinations.
// https://www.iana.org/assignments/iana-ipv6-special-registry/
// https://www.iana.org/assignments/ipv6-address-space/
fn is_public_ipv6(address: Ipv6Addr) -> bool {
    ipv6_in_prefix(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3)
        && !ipv6_in_prefix(address, Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23)
        && !ipv6_in_prefix(address, Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 32)
        && !ipv6_in_prefix(address, Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16)
        && !ipv6_in_prefix(address, Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20)
}

fn ipv6_in_prefix(address: Ipv6Addr, network: Ipv6Addr, prefix_length: u32) -> bool {
    let mask = u128::MAX << (128 - prefix_length);
    u128::from(address) & mask == u128::from(network) & mask
}

const fn retryable_dns_failure() -> ConnectorExecutionError {
    ConnectorExecutionError::Retryable { retry_after: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;

    enum LookupOutcome {
        Addresses(Vec<IpAddr>),
        Unavailable,
        Pending,
    }

    struct SequenceDnsResolver {
        outcomes: Mutex<VecDeque<LookupOutcome>>,
        names: Mutex<Vec<String>>,
        calls: AtomicUsize,
    }

    impl SequenceDnsResolver {
        fn new(outcomes: Vec<LookupOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into()),
                names: Mutex::new(Vec::new()),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl DnsIpResolver for SequenceDnsResolver {
        async fn lookup_ip(
            &self,
            absolute_name: &str,
        ) -> Result<Vec<IpAddr>, DnsIpResolutionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.names.lock().await.push(absolute_name.to_owned());
            match self
                .outcomes
                .lock()
                .await
                .pop_front()
                .unwrap_or(LookupOutcome::Unavailable)
            {
                LookupOutcome::Addresses(addresses) => Ok(addresses),
                LookupOutcome::Unavailable => Err(DnsIpResolutionError),
                LookupOutcome::Pending => future::pending().await,
            }
        }
    }

    fn policy(
        outcomes: Vec<LookupOutcome>,
        timeout: Duration,
    ) -> (
        PublicInternetConnectorEgressAuthorizer,
        Arc<SequenceDnsResolver>,
    ) {
        let resolver = Arc::new(SequenceDnsResolver::new(outcomes));
        let policy =
            PublicInternetConnectorEgressAuthorizer::with_resolver(resolver.clone(), timeout)
                .expect("egress policy");
        (policy, resolver)
    }

    fn public_endpoint() -> Url {
        Url::parse("https://hooks.acme.dev:8443/delivery?token=redacted").expect("endpoint")
    }

    #[tokio::test]
    async fn policy_resolves_an_absolute_name_and_returns_deduplicated_exact_addresses() {
        let (policy, resolver) = policy(
            vec![LookupOutcome::Addresses(vec![
                "8.8.8.8".parse().expect("address"),
                "1.1.1.1".parse().expect("address"),
                "8.8.8.8".parse().expect("address"),
            ])],
            Duration::from_millis(100),
        );
        let endpoint = public_endpoint();

        let destination = policy
            .authorize(ConnectorRevisionId::new(), &endpoint)
            .await
            .expect("public destination");

        assert!(destination.matches_endpoint(&endpoint));
        assert_eq!(
            destination.socket_addresses(),
            &[
                "1.1.1.1:8443".parse().expect("socket address"),
                "8.8.8.8:8443".parse().expect("socket address"),
            ]
        );
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
        assert_eq!(resolver.names.lock().await.as_slice(), ["hooks.acme.dev."]);
    }

    #[tokio::test]
    async fn policy_rechecks_each_attempt_and_rejects_dns_rebinding() {
        let (policy, resolver) = policy(
            vec![
                LookupOutcome::Addresses(vec!["1.1.1.1".parse().expect("public address")]),
                LookupOutcome::Addresses(vec!["127.0.0.1".parse().expect("loopback")]),
            ],
            Duration::from_millis(100),
        );
        let endpoint = public_endpoint();

        policy
            .authorize(ConnectorRevisionId::new(), &endpoint)
            .await
            .expect("first public answer");
        assert_eq!(
            policy
                .authorize(ConnectorRevisionId::new(), &endpoint)
                .await,
            Err(ConnectorExecutionError::Rejected)
        );
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn policy_rejects_mixed_private_and_oversized_answer_sets() {
        let oversized = (1..=MAXIMUM_AUTHORIZED_CONNECTOR_ADDRESSES + 1)
            .map(|last| IpAddr::V4(Ipv4Addr::new(11, 0, 0, last as u8)))
            .collect();
        let (policy, resolver) = policy(
            vec![
                LookupOutcome::Addresses(vec![
                    "8.8.8.8".parse().expect("public address"),
                    "10.0.0.1".parse().expect("private address"),
                ]),
                LookupOutcome::Addresses(oversized),
            ],
            Duration::from_millis(100),
        );
        let endpoint = public_endpoint();

        assert_eq!(
            policy
                .authorize(ConnectorRevisionId::new(), &endpoint)
                .await,
            Err(ConnectorExecutionError::Rejected)
        );
        assert_eq!(
            policy
                .authorize(ConnectorRevisionId::new(), &endpoint)
                .await,
            Err(ConnectorExecutionError::Rejected)
        );
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn special_names_non_https_and_single_labels_are_rejected_without_dns() {
        let (policy, resolver) =
            policy(vec![LookupOutcome::Unavailable], Duration::from_millis(100));
        for endpoint in [
            "https://service.localhost/path",
            "https://service.alt/path",
            "https://hooks.example.com/path",
            "https://intranet/path",
            "http://hooks.acme.dev/path",
        ] {
            assert_eq!(
                policy
                    .authorize(
                        ConnectorRevisionId::new(),
                        &Url::parse(endpoint).expect("endpoint"),
                    )
                    .await,
                Err(ConnectorExecutionError::Rejected),
                "{endpoint}"
            );
        }
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unavailable_empty_and_timed_out_dns_are_retryable_without_provider_details() {
        let (policy, resolver) = policy(
            vec![
                LookupOutcome::Unavailable,
                LookupOutcome::Addresses(Vec::new()),
                LookupOutcome::Pending,
            ],
            Duration::from_millis(5),
        );
        let endpoint = public_endpoint();

        for _ in 0..3 {
            assert_eq!(
                policy
                    .authorize(ConnectorRevisionId::new(), &endpoint)
                    .await,
                Err(ConnectorExecutionError::Retryable { retry_after: None })
            );
        }
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn public_ip_literals_skip_dns_while_special_literals_are_rejected() {
        let (policy, resolver) = policy(vec![LookupOutcome::Pending], Duration::from_millis(100));
        let public = Url::parse("https://1.1.1.1/path").expect("public endpoint");
        let destination = policy
            .authorize(ConnectorRevisionId::new(), &public)
            .await
            .expect("public literal");
        assert_eq!(
            destination.socket_addresses(),
            &["1.1.1.1:443".parse().expect("socket address")]
        );
        for endpoint in ["https://127.0.0.1/path", "https://[::1]/path"] {
            assert_eq!(
                policy
                    .authorize(
                        ConnectorRevisionId::new(),
                        &Url::parse(endpoint).expect("endpoint"),
                    )
                    .await,
                Err(ConnectorExecutionError::Rejected)
            );
        }
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn address_classifier_is_closed_over_special_and_public_ranges() {
        for address in [
            "0.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.0.0.8",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.0.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
            "::1",
            "64:ff9b::1",
            "100::1",
            "2001::1",
            "2001:db8::1",
            "2002::1",
            "3fff::1",
            "5f00::1",
            "fc00::1",
            "fe80::1",
            "ff00::1",
        ] {
            assert!(
                !is_public_ip(address.parse().expect("special address")),
                "{address}"
            );
        }
        for address in [
            "1.1.1.1",
            "8.8.8.8",
            "192.0.0.9",
            "192.0.0.10",
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
        ] {
            assert!(
                is_public_ip(address.parse().expect("public address")),
                "{address}"
            );
        }
    }

    #[test]
    fn policy_rejects_unbounded_lookup_timeouts() {
        let resolver = Arc::new(SequenceDnsResolver::new(Vec::new()));
        assert!(PublicInternetConnectorEgressAuthorizer::with_resolver(
            resolver.clone(),
            Duration::ZERO,
        )
        .is_err());
        assert!(PublicInternetConnectorEgressAuthorizer::with_resolver(
            resolver,
            MAXIMUM_DNS_LOOKUP_TIMEOUT + Duration::from_millis(1),
        )
        .is_err());
    }
}
