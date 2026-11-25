use serde::Serialize;

/// 파일 단위 실패 사유. JSON의 `error.code`에 그대로 나간다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    UnsupportedFormat,
    EmptyFile,
    TooLarge,
    /// ffmpeg가 오디오를 못 뽑음 (ffmpeg가 없는 경우 포함)
    DecodeFailed,
    ModelNotFound,
    TranscribeFailed,
    NoSpeech,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct TranscribeError {
    pub code: ErrorCode,
    pub message: String,
}

impl TranscribeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
