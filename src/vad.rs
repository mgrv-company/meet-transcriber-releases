//! 말이 있는 구간 찾기(Silero VAD, whisper.cpp 내장 구현).

use crate::error::{ErrorCode, TranscribeError};
use std::path::Path;
use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

pub struct SpeechDetector {
    ctx: WhisperVadContext,
    max_speech_s: f32,
    threshold: f32,
    min_speech_ms: i32,
}

impl SpeechDetector {
    pub fn new(
        vad_model: &Path,
        threads: Option<i32>,
        threshold: f32,
        min_speech_ms: i32,
    ) -> Result<Self, TranscribeError> {
        if !vad_model.is_file() {
            return Err(TranscribeError::new(
                ErrorCode::ModelNotFound,
                format!("VAD 모델 파일이 없습니다: {}", vad_model.display()),
            ));
        }
        let mut p = WhisperVadContextParams::new();
        p.set_use_gpu(false);
        if let Some(t) = threads {
            p.set_n_threads(t);
        }
        let path = vad_model
            .to_str()
            .ok_or_else(|| TranscribeError::new(ErrorCode::ModelNotFound, "VAD 모델 경로를 읽을 수 없습니다"))?;
        let ctx = WhisperVadContext::new(path, p)
            .map_err(|e| TranscribeError::new(ErrorCode::ModelNotFound, format!("VAD 모델을 열지 못했습니다: {e}")))?;
        Ok(Self {
            ctx,
            max_speech_s: 30.0,
            threshold,
            min_speech_ms,
        })
    }

    /// 16kHz 모노 샘플에서 (시작, 끝) 초 구간을 돌려준다. 시각은 샘플 시작점 기준이다.
    pub fn detect(&mut self, samples: &[f32]) -> Result<Vec<(f64, f64)>, TranscribeError> {
        let mut params = WhisperVadParams::new();
        params.set_max_speech_duration(self.max_speech_s);
        params.set_threshold(self.threshold);
        params.set_min_speech_duration(self.min_speech_ms);
        let segs = self
            .ctx
            .segments_from_samples(params, samples)
            .map_err(|e| TranscribeError::new(ErrorCode::TranscribeFailed, format!("VAD 실패: {e}")))?;
        // whisper-rs 문서상 시각 단위는 센티초(10ms).
        Ok(segs.map(|s| (s.start as f64 / 100.0, s.end as f64 / 100.0)).collect())
    }
}
