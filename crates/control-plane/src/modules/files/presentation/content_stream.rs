use crate::modules::files::application::UserFileObjectReader;
use a3s_boot::{BootError, BootResponse, Result, StreamableFile};
use futures_util::StreamExt;
use tokio_util::io::ReaderStream;

pub(crate) fn stream_user_file_content(
    media_type: String,
    size_bytes: u64,
    reader: UserFileObjectReader,
) -> Result<BootResponse> {
    let stream = ReaderStream::new(reader).map(|chunk| match chunk {
        Ok(bytes) => Ok(bytes.to_vec()),
        Err(error) => Err(BootError::Internal(format!(
            "failed while streaming admitted UserFile content: {error}"
        ))),
    });
    Ok(BootResponse::streamable_file(
        StreamableFile::stream(stream)
            .with_content_type(media_type)
            .with_content_length(size_bytes),
    ))
}
