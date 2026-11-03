use rmcp::model::{CallToolResult, ContentBlock};
use thirtyfour::error::WebDriverError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("no active browser session — call session_start first")]
    NoActiveSession,
    #[error("a browser session is already active — call session_end before starting a new one")]
    SessionAlreadyActive,
    #[error("browser session error: {0}")]
    Driver(#[from] WebDriverError),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    InvalidTarget(String),
    #[error("invalid profile name '{0}' — use only letters, digits, '_', or '-'")]
    InvalidProfileName(String),
    #[error("ffmpeg error: {0}")]
    Ffmpeg(String),
    #[error("a recording is already active — call record_stop before starting a new one")]
    RecordingAlreadyActive,
    #[error("no active recording — call record_start first")]
    NoActiveRecording,
    #[error("embedding error: {0}")]
    Embedding(String),
    #[error("semantic memory is unavailable: {0}")]
    ChromaUnavailable(String),
    #[error("Chroma error: {0}")]
    Chroma(String),
}

impl AppError {
    pub fn into_tool_result(self) -> CallToolResult {
        CallToolResult::error(vec![ContentBlock::text(self.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_active_session_message_names_the_fix() {
        let err = AppError::NoActiveSession;
        assert_eq!(
            err.to_string(),
            "no active browser session — call session_start first"
        );
    }

    #[test]
    fn session_already_active_message_names_the_fix() {
        let err = AppError::SessionAlreadyActive;
        assert_eq!(
            err.to_string(),
            "a browser session is already active — call session_end before starting a new one"
        );
    }

    #[test]
    fn invalid_profile_name_message_names_the_offending_value() {
        let err = AppError::InvalidProfileName("bad name".to_string());
        assert!(err.to_string().contains("bad name"));
    }

    #[test]
    fn into_tool_result_marks_the_result_as_an_error_with_the_message_as_content() {
        let result = AppError::SessionAlreadyActive.into_tool_result();

        assert_eq!(result.is_error, Some(true));
        let text = result.content[0].as_text().expect("expected text content");
        assert_eq!(
            text.text,
            "a browser session is already active — call session_end before starting a new one"
        );
    }
}
