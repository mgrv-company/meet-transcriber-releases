//! 전사 엔진. 엔진을 바꿔 끼울 수 있게 trait 뒤에 둔다.

pub mod gpu;
pub mod whisper;

use crate::error::TranscribeError;
use std::path::PathBuf;

/// 16kHz 모노 f32 샘플 한 덩어리를 글로 바꾼다. 내용을 고치지 않는다.
pub trait SpeechEngine {
    fn model_name(&self) -> &str;
    fn transcribe(&mut self, samples: &[f32]) -> Result<Transcript, String>;
}

/// 설정으로 엔진을 연다. 어떤 모델·런타임을 쓸지 고르는 곳은 여기 하나뿐이다.
/// 지금은 whisper.cpp(ggml) 파일만 다룬다.
pub fn open(cfg: &EngineConfig) -> Result<Box<dyn SpeechEngine>, TranscribeError> {
    Ok(Box::new(whisper::WhisperEngine::new(cfg)?))
}

/// 엔진을 열 때 필요한 값. 모델 종류에 따라 다른 값이 필요해지면 여기에 더한다.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub model_path: PathBuf,
    pub language: String,
    pub threads: Option<i32>,
    pub use_gpu: bool,
    pub flash_attn: bool,
    pub clean_fragments: bool,
}

/// 엔진이 샘플 한 덩어리에서 돌려주는 결과.
#[derive(Debug, Default)]
pub struct Transcript {
    pub text: String,
    /// 말소리가 아닌 데서 지어낸 것으로 보여 통째로 뺀 조각의 글
    pub dropped_segments: Vec<String>,
    /// 한두 글자 영어 조각(`o.`, `ue` 등)으로 보아 글에서 뺀 낱말
    pub removed_fragments: Vec<String>,
    /// 남긴 글의 토큰별 확률 평균(0~1). 낮을수록 모델이 확신하지 못한 글이다
    pub confidence: Option<f32>,
    /// 글자가 깨져 대체 문자(U+FFFD)로 남은 수
    pub broken_chars: usize,
}

/// whisper.cpp와 ggml이 stderr로 직접 내던 로그를 Rust의 `log`로 보낸다. 어느 로거도 정하지 않으면 버려진다.
/// 엔진은 디버그 줄에 전사 중인 글을 찍으므로, 모델을 열기 전에 한 번 부른다.
pub fn route_engine_logs() {
    whisper_rs::install_logging_hooks();
}
