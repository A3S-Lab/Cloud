use lettre::address::{Address, Envelope};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{
    AsyncSmtpConnection, Certificate, TlsParameters, TlsVersion,
};
use lettre::transport::smtp::extension::ClientId;
use std::path::Path;
use std::time::Duration;
use zeroize::Zeroizing;

const MAX_CA_CERTIFICATE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SmtpTlsPolicy {
    RequiredStartTls,
    Implicit,
}

pub(crate) struct SmtpCredentials {
    pub username: Zeroizing<String>,
    pub password: Zeroizing<String>,
}

impl std::fmt::Debug for SmtpCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmtpCredentials")
            .field("username", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

pub(crate) struct SmtpTransportOptions {
    pub host: String,
    pub port: u16,
    pub tls_policy: SmtpTlsPolicy,
    pub hello_name: String,
    pub ca_certificate_file: String,
    pub credentials: SmtpCredentials,
    pub connect_timeout: Duration,
    pub command_timeout: Duration,
}

pub(crate) struct SmtpTransport {
    host: String,
    port: u16,
    tls_policy: SmtpTlsPolicy,
    hello_name: ClientId,
    credentials: SmtpCredentials,
    tls_parameters: TlsParameters,
    connect_timeout: Duration,
    command_timeout: Duration,
}

impl SmtpTransport {
    pub(crate) fn new(options: SmtpTransportOptions) -> Result<Self, String> {
        if options.host.is_empty()
            || options.host.len() > 253
            || options.port == 0
            || options.hello_name.is_empty()
            || options.hello_name.len() > 253
            || options.credentials.username.is_empty()
            || options.credentials.username.len() > 1024
            || options.credentials.password.is_empty()
            || options.credentials.password.len() > 8192
            || options.connect_timeout.is_zero()
            || options.connect_timeout > Duration::from_secs(60)
            || options.command_timeout.is_zero()
            || options.command_timeout > Duration::from_secs(60)
        {
            return Err("SMTP transport options are invalid".into());
        }

        let mut tls =
            TlsParameters::builder(options.host.clone()).set_min_tls_version(TlsVersion::Tlsv12);
        if !options.ca_certificate_file.is_empty() {
            let path = Path::new(&options.ca_certificate_file);
            let metadata = std::fs::metadata(path)
                .map_err(|_| "could not inspect SMTP CA certificate file".to_owned())?;
            if !metadata.is_file()
                || metadata.len() == 0
                || metadata.len() > MAX_CA_CERTIFICATE_BYTES
            {
                return Err("SMTP CA certificate file is invalid".into());
            }
            let pem = std::fs::read(path)
                .map_err(|_| "could not read SMTP CA certificate file".to_owned())?;
            let certificate = Certificate::from_pem(&pem)
                .map_err(|_| "SMTP CA certificate file is invalid".to_owned())?;
            tls = tls.add_root_certificate(certificate);
        }
        let tls_parameters = tls
            .build()
            .map_err(|_| "could not construct SMTP TLS policy".to_owned())?;

        Ok(Self {
            host: options.host,
            port: options.port,
            tls_policy: options.tls_policy,
            hello_name: ClientId::Domain(options.hello_name),
            credentials: options.credentials,
            tls_parameters,
            connect_timeout: options.connect_timeout,
            command_timeout: options.command_timeout,
        })
    }

    pub(crate) async fn prepare(&self) -> Result<PreparedSmtpSession, SmtpPreparationError> {
        let implicit_tls =
            (self.tls_policy == SmtpTlsPolicy::Implicit).then(|| self.tls_parameters.clone());
        let connection = tokio::time::timeout(
            self.connect_timeout,
            AsyncSmtpConnection::connect_tokio1(
                (self.host.as_str(), self.port),
                Some(self.connect_timeout),
                &self.hello_name,
                implicit_tls,
                None,
            ),
        )
        .await
        .map_err(|_| SmtpPreparationError::Unavailable)?
        .map_err(|_| SmtpPreparationError::Unavailable)?;
        let mut connection = connection;

        if self.tls_policy == SmtpTlsPolicy::RequiredStartTls {
            if !connection.can_starttls() {
                return Err(SmtpPreparationError::Unavailable);
            }
            tokio::time::timeout(
                self.command_timeout,
                connection.starttls(self.tls_parameters.clone(), &self.hello_name),
            )
            .await
            .map_err(|_| SmtpPreparationError::Unavailable)?
            .map_err(|_| SmtpPreparationError::Unavailable)?;
        }
        if !connection.is_encrypted() {
            return Err(SmtpPreparationError::Unavailable);
        }

        let credentials = Credentials::new(
            self.credentials.username.to_string(),
            self.credentials.password.to_string(),
        );
        tokio::time::timeout(
            self.command_timeout,
            connection.auth(&[Mechanism::Plain, Mechanism::Login], &credentials),
        )
        .await
        .map_err(|_| SmtpPreparationError::Unavailable)?
        .map_err(|_| SmtpPreparationError::Unavailable)?;
        drop(credentials);

        Ok(PreparedSmtpSession {
            connection,
            command_timeout: self.command_timeout,
        })
    }
}

impl std::fmt::Debug for SmtpTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmtpTransport")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls_policy", &self.tls_policy)
            .field("credentials", &"[REDACTED]")
            .field("connect_timeout", &self.connect_timeout)
            .field("command_timeout", &self.command_timeout)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SmtpPreparationError {
    Unavailable,
}

pub(crate) struct PreparedSmtpSession {
    connection: AsyncSmtpConnection,
    command_timeout: Duration,
}

impl PreparedSmtpSession {
    pub(crate) fn prepare_submission(
        self,
        sender: &str,
        recipient: &str,
        message: Zeroizing<Vec<u8>>,
    ) -> Result<PreparedSmtpSubmission, SmtpSubmissionPreparationError> {
        if message.is_empty() {
            return Err(SmtpSubmissionPreparationError::Invalid);
        }
        let sender = sender
            .parse::<Address>()
            .map_err(|_| SmtpSubmissionPreparationError::Invalid)?;
        let recipient = recipient
            .parse::<Address>()
            .map_err(|_| SmtpSubmissionPreparationError::Invalid)?;
        let envelope = Envelope::new(Some(sender), vec![recipient])
            .map_err(|_| SmtpSubmissionPreparationError::Invalid)?;
        Ok(PreparedSmtpSubmission {
            connection: self.connection,
            envelope,
            message,
            command_timeout: self.command_timeout,
        })
    }
}

impl std::fmt::Debug for PreparedSmtpSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedSmtpSession")
            .field("connection", &"[REDACTED]")
            .field("command_timeout", &self.command_timeout)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SmtpSubmissionPreparationError {
    Invalid,
}

pub(crate) struct PreparedSmtpSubmission {
    connection: AsyncSmtpConnection,
    envelope: Envelope,
    message: Zeroizing<Vec<u8>>,
    command_timeout: Duration,
}

impl PreparedSmtpSubmission {
    pub(crate) async fn submit(mut self) -> SmtpSubmissionOutcome {
        match tokio::time::timeout(
            self.command_timeout,
            self.connection
                .send(&self.envelope, self.message.as_slice()),
        )
        .await
        {
            Ok(Ok(_)) => SmtpSubmissionOutcome::Accepted,
            Ok(Err(error)) if error.is_permanent() => SmtpSubmissionOutcome::PermanentRejected,
            Ok(Err(error)) if error.is_transient() => SmtpSubmissionOutcome::TransientRejected,
            Ok(Err(_)) | Err(_) => SmtpSubmissionOutcome::Indeterminate,
        }
    }
}

impl std::fmt::Debug for PreparedSmtpSubmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedSmtpSubmission")
            .field("connection", &"[REDACTED]")
            .field("envelope", &"[REDACTED]")
            .field("message", &"[REDACTED]")
            .field("command_timeout", &self.command_timeout)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SmtpSubmissionOutcome {
    Accepted,
    PermanentRejected,
    TransientRejected,
    Indeterminate,
}
