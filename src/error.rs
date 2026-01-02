use thiserror::Error;

#[derive(Error, Debug)]
pub enum YouSummaryError {
    #[error("Invalid YouTube URL: {0}")]
    InvalidUrl(String),

    #[error("Video not found: {0}")]
    VideoNotFound(String),

    #[error("Transcript not found for video: {0}")]
    TranscriptNotFound(String),

    #[error("No videos found in the provided target")]
    NoVideosFound,

    #[error("Failed to fetch transcript: {0}")]
    TranscriptFetchError(String),

    #[error("LLM error: {0}")]
    LlmError(String),

    #[error("Invalid model format: {0}. Expected 'provider/model' (e.g., 'ollama/llama3.2:3b')")]
    InvalidModelFormat(String),

    #[error("Unsupported LLM provider: {0}")]
    UnsupportedProvider(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("File error: {0}")]
    FileError(String),

    #[error("Playlist fetch error: {0}")]
    PlaylistError(String),
}

impl From<reqwest::Error> for YouSummaryError {
    fn from(err: reqwest::Error) -> Self {
        YouSummaryError::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for YouSummaryError {
    fn from(err: std::io::Error) -> Self {
        YouSummaryError::FileError(err.to_string())
    }
}

impl From<serde_json::Error> for YouSummaryError {
    fn from(err: serde_json::Error) -> Self {
        YouSummaryError::ParseError(err.to_string())
    }
}

impl From<url::ParseError> for YouSummaryError {
    fn from(err: url::ParseError) -> Self {
        YouSummaryError::InvalidUrl(err.to_string())
    }
}
