use super::api_response_interceptor::{boot_error_response, private_no_store};
use a3s_boot::{
    BootError, BootRequest, BootResponse, BoxFuture, ExceptionFilter, ExecutionContext, Result,
};
use uuid::Uuid;
use zeroize::Zeroizing;

pub(crate) const MAX_OAUTH_QUERY_BYTES: usize = 4096;

pub(crate) struct OAuthCallbackQuery {
    pub code: Option<Zeroizing<String>>,
    pub state: Option<Zeroizing<String>>,
    pub has_error: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OAuthNoStoreErrorFilter;

impl ExceptionFilter for OAuthNoStoreErrorFilter {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        let request_id = context
            .request
            .header("x-request-id")
            .and_then(|value| Uuid::parse_str(value).ok())
            .unwrap_or_else(Uuid::new_v4);
        Box::pin(async move {
            boot_error_response(error, request_id)
                .map(oauth_no_store)
                .map(Some)
        })
    }
}

pub(crate) fn oauth_no_store(response: BootResponse) -> BootResponse {
    private_no_store(response)
}

pub(crate) fn oauth_callback_query(
    request: &BootRequest,
    label: &str,
) -> Result<OAuthCallbackQuery> {
    let mut code = None;
    let mut state = None;
    let mut has_error = false;
    for (name, value) in bounded_oauth_query_pairs(request, label)? {
        match name.as_str() {
            "code" => set_once(&mut code, Zeroizing::new(value), label, "code")?,
            "state" => set_once(&mut state, Zeroizing::new(value), label, "state")?,
            "error" if has_error => {
                return Err(BootError::BadRequest(format!(
                    "{label} authorization error parameter is duplicated"
                )))
            }
            "error" => has_error = true,
            _ => {}
        }
    }
    Ok(OAuthCallbackQuery {
        code,
        state,
        has_error,
    })
}

pub(crate) fn bounded_oauth_query_pairs(
    request: &BootRequest,
    label: &str,
) -> Result<Vec<(String, String)>> {
    let Some(query) = request.query_string() else {
        return request.query_pairs();
    };
    if query.len() > MAX_OAUTH_QUERY_BYTES {
        return Err(BootError::BadRequest(format!("{label} query is too large")));
    }
    Ok(url::form_urlencoded::parse(query.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect())
}

fn set_once<T>(slot: &mut Option<T>, value: T, label: &str, parameter: &str) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(BootError::BadRequest(format!(
            "{label} {parameter} parameter is duplicated"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_query_is_bounded_and_rejects_duplicates() {
        let duplicate =
            BootRequest::new(a3s_boot::HttpMethod::Get, "/callback?state=one&state=two");
        assert!(oauth_callback_query(&duplicate, "OIDC").is_err());

        let oversized = BootRequest::new(
            a3s_boot::HttpMethod::Get,
            format!("/callback?state={}", "x".repeat(MAX_OAUTH_QUERY_BYTES)),
        );
        assert!(oauth_callback_query(&oversized, "OIDC").is_err());
    }
}
