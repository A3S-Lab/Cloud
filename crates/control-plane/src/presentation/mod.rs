mod api_contract;
mod api_response_interceptor;
mod management_mcp;
mod polling_sse;
mod request_id_middleware;
mod sequence_stream;

pub use api_contract::{
    generate_openapi_contract, openapi_info, ApiContractModule, API_CONTRACT_VERSION_HEADER,
    API_MAJOR_VERSION, API_PREFIX, MINIMUM_DEPRECATION_DAYS, OPENAPI_CONTRACT_VERSION,
    OPENAPI_DOCUMENT_PATH, OPENAPI_PUBLIC_PATH,
};
pub(crate) use api_response_interceptor::{
    api_success_envelope, application_error_envelope, boot_error_response,
};
pub use api_response_interceptor::{
    application_error_response, ApiErrorFilter, ApiResponseInterceptor,
};
pub use management_mcp::{ManagementMcpModule, MANAGEMENT_MCP_PROTOCOL_VERSION};
pub(crate) use polling_sse::{polling_sse_stream, PollingSseInitial, PollingSseOptions};
pub use request_id_middleware::RequestIdMiddleware;
pub(crate) use sequence_stream::{
    decode_sequence_cursor, default_live_sequence_limit, format_sequence_cursor,
    parse_sequence_cursor, resolve_sequence_cursor, sequence_stream_error, stream_sequence_pages,
    SequencePage, SequenceRecord, MAX_LIVE_SEQUENCE_RECORDS,
};
