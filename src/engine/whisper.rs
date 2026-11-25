use super::{EngineConfig, SpeechEngine, Transcript};
use crate::clean::strip_noise_fragments;
use crate::error::{ErrorCode, TranscribeError};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperSegment, WhisperState,
};

pub struct WhisperEngine {
    name: String,
    language: String,
    threads: Option<i32>,
    clean_fragments: bool,
    // state가 ctx를 참조하므로 ctx를 같이 들고 있는다.
    _ctx: WhisperContext,
    state: WhisperState,
}

impl WhisperEngine {
    pub fn new(cfg: &EngineConfig) -> Result<Self, TranscribeError> {
        let model = cfg.model_path.as_path();
        if !model.is_file() {
            return Err(TranscribeError::new(
                ErrorCode::ModelNotFound,
                format!("모델 파일이 없습니다: {}", model.display()),
            ));
        }
        let mut cp = WhisperContextParameters::default();
        cp.use_gpu(cfg.use_gpu);
        cp.flash_attn(cfg.flash_attn);
        let ctx = WhisperContext::new_with_params(model, cp)
            .map_err(|e| TranscribeError::new(ErrorCode::ModelNotFound, format!("모델을 열지 못했습니다: {e}")))?;
        let state = ctx.create_state().map_err(|e| {
            TranscribeError::new(
                ErrorCode::TranscribeFailed,
                format!("전사 상태를 만들지 못했습니다: {e}"),
            )
        })?;
        let name = model
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "whisper".into());
        Ok(Self {
            name,
            language: cfg.language.clone(),
            threads: cfg.threads,
            clean_fragments: cfg.clean_fragments,
            _ctx: ctx,
            state,
        })
    }
}

impl SpeechEngine for WhisperEngine {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<Transcript, String> {
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some(self.language.as_str()));
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        // 앞 구간 글을 다음 구간에 끌고 가면 지어낸 문장이 번지므로 구간마다 끊는다.
        p.set_no_context(true);
        p.set_suppress_nst(true);
        if let Some(t) = self.threads {
            p.set_n_threads(t);
        }
        self.state.full(p, samples).map_err(|e| e.to_string())?;

        let segments = self.state.as_iter().map(read_segment).collect::<Result<Vec<_>, _>>()?;
        Ok(assemble(segments, self.clean_fragments))
    }
}

/// 모델이 낸 조각 하나. 글은 바이트로 들고 있다가 이어 붙인 뒤에 한 번만 해석한다.
#[derive(Debug, Clone)]
struct RawSegment {
    bytes: Vec<u8>,
    /// 말소리가 아닐 확률(디버그 출력용)
    no_speech_prob: f32,
    /// 토큰별 확률의 합과 토큰 수(특수 토큰은 뺀다)
    prob_sum: f32,
    tokens: u32,
}

impl RawSegment {
    fn avg_prob(&self) -> f32 {
        if self.tokens > 0 {
            self.prob_sum / self.tokens as f32
        } else {
            0.0
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

/// 조각 안 토큰들의 평균 확률이 이보다 낮으면 말소리가 아닌 데서 지어낸 것으로 본다.
/// 한국어 녹음 150초 표본에서 지어낸 조각은 0.07~0.08, 정상 조각은 0.22 이상이었다.
const MIN_AVG_TOKEN_PROB: f32 = 0.15;

/// 모델 상태에서 조각 하나를 읽는다. 토큰 확률의 평균을 내기 위한 합과 개수를 함께 모은다.
fn read_segment(seg: WhisperSegment<'_>) -> Result<RawSegment, String> {
    let speech_probs = (0..seg.n_tokens())
        .filter_map(|i| seg.get_token(i))
        .map(|tok| {
            let text = tok.to_str_lossy().map_err(|e| e.to_string())?;
            Ok::<_, String>((!text.starts_with("[_") && !text.starts_with("<|")).then(|| tok.token_probability()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (prob_sum, tokens) = speech_probs
        .into_iter()
        .flatten()
        .fold((0.0f32, 0u32), |(sum, n), p| (sum + p, n + 1));
    Ok(RawSegment {
        bytes: seg.to_bytes().map_err(|e| e.to_string())?.to_vec(),
        no_speech_prob: seg.no_speech_probability(),
        prob_sum,
        tokens,
    })
}

/// 모델이 낸 조각들에서 말이 아닌 것을 가려내고 글 하나로 합친다. 바깥을 건드리지 않는다.
fn assemble(segments: Vec<RawSegment>, clean_fragments: bool) -> Transcript {
    let (kept, dropped): (Vec<_>, Vec<_>) = segments
        .into_iter()
        .partition(|s| !is_probably_not_speech(s.avg_prob(), &s.text()));
    let dropped_segments = dropped.iter().map(|s| s.text().trim().to_string()).collect();
    let (kept_sum, kept_n) = kept
        .iter()
        .fold((0.0f32, 0u32), |(sum, n), s| (sum + s.prob_sum, n + s.tokens));
    // 한 글자가 여러 조각에 걸쳐 잘릴 수 있어, 바이트를 모두 이은 뒤에 한 번만 해석한다.
    let bytes: Vec<u8> = kept.iter().flat_map(|s| s.bytes.iter().copied()).collect();
    let joined = String::from_utf8_lossy(&bytes).trim().to_string();
    let (text, removed_fragments) = if clean_fragments {
        strip_noise_fragments(&joined)
    } else {
        (joined, Vec::new())
    };
    Transcript {
        broken_chars: text.matches('\u{fffd}').count(),
        confidence: (kept_n > 0).then(|| kept_sum / kept_n as f32),
        text,
        dropped_segments,
        removed_fragments,
    }
}

/// 확신이 매우 낮고, 한글·숫자·문장부호 밖의 글자(영어, 다른 나라 글자, 깨진 글자)가 섞인 조각만 뺀다.
/// 한글만 있는 짧은 말(「그러면」「자」 같은 맞장구)은 확신이 낮아도 남긴다.
fn is_probably_not_speech(avg_token_prob: f32, text: &str) -> bool {
    avg_token_prob < MIN_AVG_TOKEN_PROB && !is_korean_only(text)
}

fn is_korean_only(text: &str) -> bool {
    let mut has_hangul = false;
    for c in text.chars() {
        let hangul = matches!(c, '\u{AC00}'..='\u{D7A3}' | '\u{3131}'..='\u{318E}');
        has_hangul |= hangul;
        if !(hangul
            || c.is_whitespace()
            || c.is_ascii_digit()
            || c.is_ascii_punctuation()
            || matches!(c, '…' | '·' | '“' | '”' | '‘' | '’'))
        {
            return false;
        }
    }
    has_hangul
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, prob_sum: f32, tokens: u32) -> RawSegment {
        RawSegment {
            bytes: text.as_bytes().to_vec(),
            no_speech_prob: 0.0,
            prob_sum,
            tokens,
        }
    }

    #[test]
    fn keeps_short_korean_even_when_unsure() {
        assert!(!is_probably_not_speech(0.05, "그러면"));
        assert!(!is_probably_not_speech(0.05, "자."));
    }

    #[test]
    fn drops_unsure_mixed_script_garbage() {
        assert!(is_probably_not_speech(0.08, "ui Prija ottira bGd"));
        assert!(is_probably_not_speech(0.07, "하고 neloca �araus 했을 때"));
    }

    #[test]
    fn keeps_confident_text() {
        assert!(!is_probably_not_speech(0.8, "PM이랑 RED랑"));
    }

    #[test]
    fn assemble_drops_garbage_and_averages_over_kept_only() {
        let segs = vec![
            seg(" 안녕하세요", 1.6, 2),
            seg(" ui Prija bGd", 0.3, 4),
            seg(" 반갑습니다", 1.4, 2),
        ];
        let t = assemble(segs, true);
        assert_eq!(t.text, "안녕하세요 반갑습니다");
        assert_eq!(t.dropped_segments, vec!["ui Prija bGd"]);
        // 남긴 두 조각만: (1.6 + 1.4) / 4
        assert!((t.confidence.unwrap() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn assemble_strips_fragments_only_when_asked() {
        let on = assemble(vec![seg(" o. 네 알겠습니다", 1.6, 2)], true);
        assert_eq!(on.text, "네 알겠습니다");
        assert_eq!(on.removed_fragments, vec!["o."]);
        let off = assemble(vec![seg(" o. 네 알겠습니다", 1.6, 2)], false);
        assert_eq!(off.text, "o. 네 알겠습니다");
        assert!(off.removed_fragments.is_empty());
    }

    #[test]
    fn assemble_joins_bytes_before_decoding() {
        // 한 글자("가" = EA B0 80)가 두 조각에 걸쳐 잘려도 깨지지 않는다.
        let bytes = "가".as_bytes();
        let a = RawSegment {
            bytes: bytes[..2].to_vec(),
            no_speech_prob: 0.0,
            prob_sum: 0.9,
            tokens: 1,
        };
        let b = RawSegment {
            bytes: bytes[2..].to_vec(),
            no_speech_prob: 0.0,
            prob_sum: 0.9,
            tokens: 1,
        };
        let t = assemble(vec![a, b], true);
        assert_eq!(t.text, "가");
        assert_eq!(t.broken_chars, 0);
    }

    #[test]
    fn assemble_without_tokens_has_no_confidence() {
        let t = assemble(Vec::new(), true);
        assert!(t.text.is_empty());
        assert!(t.confidence.is_none());
    }
}
