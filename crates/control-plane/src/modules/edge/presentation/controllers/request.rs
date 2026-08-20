use crate::presentation::A3S_ACL_MEDIA_TYPE;
pub(super) use crate::presentation::{request_id, request_identity};
use a3s_boot::{BootError, BootRequest, Result};

pub(super) fn acl_document(
    request: &BootRequest,
    maximum_bytes: usize,
    label: &str,
) -> Result<String> {
    let media_type = request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !media_type.is_some_and(|value| value.eq_ignore_ascii_case(A3S_ACL_MEDIA_TYPE)) {
        return Err(BootError::UnsupportedMediaType(format!(
            "{label} requires {A3S_ACL_MEDIA_TYPE}"
        )));
    }
    if request.body().is_empty() {
        return Err(BootError::BadRequest(format!(
            "{label} ACL body is required"
        )));
    }
    if request.body().len() > maximum_bytes {
        return Err(BootError::PayloadTooLarge(format!(
            "{label} ACL exceeds {maximum_bytes} bytes"
        )));
    }
    std::str::from_utf8(request.body())
        .map(str::to_owned)
        .map_err(|_| BootError::BadRequest(format!("{label} ACL must be valid UTF-8")))
}
