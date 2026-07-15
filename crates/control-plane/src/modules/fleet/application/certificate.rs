use crate::modules::fleet::domain::entities::NodeCertificate;
use crate::modules::fleet::domain::services::CertificateAuthorityError;
use crate::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_contracts::NodeCertificate as NodeCertificateContract;

pub(super) fn contract(certificate: &NodeCertificate) -> NodeCertificateContract {
    NodeCertificateContract {
        certificate_id: certificate.id.as_uuid(),
        serial_number: certificate.serial_number.clone(),
        certificate_pem: certificate.certificate_pem.clone(),
        ca_bundle_pem: certificate.ca_bundle_pem.clone(),
        issued_at: certificate.issued_at,
        expires_at: certificate.expires_at,
    }
}

pub(super) fn application_error(error: CertificateAuthorityError) -> ApplicationError {
    match error {
        CertificateAuthorityError::InvalidRequest(message) => ApplicationError::Invalid(message),
        CertificateAuthorityError::Rejected(message) => ApplicationError::Forbidden(message),
        CertificateAuthorityError::Unavailable(message) => ApplicationError::Internal(message),
    }
}
