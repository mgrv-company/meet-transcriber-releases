//! 데스크톱 앱 안 전사 사이드카. 앱(Electron 메인 프로세스)이 띄우고 표준 입출력으로 한 줄에 JSON 하나씩 주고받는다.
//! 약속:
//!   사이드카 → 앱: 시작하면 `ready{gpu}` 한 번, `audio`마다 `segments{seq,segments}`, `finish`에 `done{segments}` 뒤 끝.
//!   앱 → 사이드카: `audio{seq,sampleRate,channels,encoding,dataBase64}`, `finish`.
//! 소리는 메모리에만 두고 파일로 쓰지 않으며 밖으로 보내지 않는다. 진단 글은 표준 오류로만 낸다(앱은 읽지 않는다).

mod clean;
mod engine;
mod error;
mod lines;
mod vad;
mod window;

use base64::Engine as _;
use engine::{gpu, EngineConfig, SpeechEngine};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use vad::SpeechDetector;
use window::{plan, Plan, SAMPLE_RATE, SENTENCE_GAP_SEC, TAIL_GUARD_SEC};

fn main() -> ExitCode {
    engine::route_engine_logs();
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => return fail(&message),
    };
    let config = EngineConfig {
        model_path: args.model.clone(),
        language: args.language.clone(),
        threads: None,
        use_gpu: true,
        flash_attn: false,
        clean_fragments: true,
    };
    let mut speech = match engine::open(&config) {
        Ok(speech) => speech,
        Err(e) => return fail(&format!("모델을 열지 못했습니다: {}", e.message)),
    };
    let mut vad = match SpeechDetector::new(&args.vad_model, None, 0.5, 250) {
        Ok(vad) => vad,
        Err(e) => return fail(&format!("VAD 모델을 열지 못했습니다: {}", e.message)),
    };
    // 장치 목록은 엔진을 연 뒤에야 채워진다. GPU를 못 잡으면 조용히 CPU로 내려가지 않고 앱에 알린다.
    let devices = gpu::devices();
    let gpu_status = match (gpu::GPU_BUILD, devices.is_empty()) {
        (false, _) => Gpu::Unavailable {
            reason: "GPU 지원 없이 빌드됨".into(),
        },
        (true, true) => Gpu::Unavailable {
            reason: "GPU 장치를 찾지 못함".into(),
        },
        (true, false) => Gpu::Used {
            device: devices.join(", "),
        },
    };
    if send(&Response::Ready { gpu: gpu_status }).is_err() {
        return ExitCode::from(1);
    }

    let mut stream = Stream::new(args.batch_sec);
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            return fail("표준 입력을 읽지 못했습니다");
        };
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(e) => return fail(&format!("약속과 다른 요청입니다: {e}")),
        };
        match request {
            Request::Audio {
                seq,
                sample_rate,
                channels,
                encoding,
                data_base64,
            } => {
                if sample_rate != SAMPLE_RATE as u32 || channels != 1 {
                    return fail("16kHz 모노 소리만 받습니다");
                }
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data_base64)
                else {
                    return fail("소리 조각의 base64를 읽지 못했습니다");
                };
                stream.push(&decode(&bytes, encoding));
                let segments = match stream.advance(false, speech.as_mut(), &mut vad) {
                    Ok(segments) => segments,
                    Err(message) => return fail(&message),
                };
                if send(&Response::Segments { seq, segments }).is_err() {
                    return ExitCode::from(1);
                }
            }
            Request::Finish {} => {
                let segments = match stream.advance(true, speech.as_mut(), &mut vad) {
                    Ok(segments) => segments,
                    Err(message) => return fail(&message),
                };
                stream.stats.report();
                return if send(&Response::Done { segments }).is_ok() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                };
            }
        }
    }
    // 앱이 finish 없이 입력을 닫았다(앱 종료, 회의 취소). 남은 소리는 버린다.
    ExitCode::SUCCESS
}

/// 녹음 처음부터 받은 소리 중 아직 확정하지 않은 부분(`pending`, 녹음 처음에서 `pos`번째 샘플부터).
struct Stream {
    pending: Vec<f32>,
    pos: usize,
    batch_samples: usize,
    /// 마지막으로 말 구간을 찾았을 때의 `pending` 길이. 새 소리가 조금 쌓일 때마다만 다시 찾는다.
    checked: usize,
    stats: Stats,
}

/// 앱이 로그에 남길 숫자 요약(회의 내용은 담지 않는다). 녹음 한 번이 끝날 때 표준 오류로 한 줄 낸다.
#[derive(Default)]
struct Stats {
    /// 앱에서 받은 소리 길이(샘플)
    received: usize,
    /// 전사에 넣은 말 구간 길이 합(샘플)
    speech: usize,
    /// 전사한 묶음 수와 그중 글이 나온 수
    groups: usize,
    texted: usize,
    /// 마지막으로 전사한 묶음의 소리 길이(샘플)
    last_group: usize,
}

/// 묶음 길이를 기다리지 않고 문장 단위로 확정할 수 있는지 이만큼 새 소리가 쌓일 때마다 본다.
const CHECK_STEP_SAMPLES: usize = SAMPLE_RATE / 2;

impl Stream {
    fn new(batch_sec: f64) -> Self {
        Self {
            pending: Vec::new(),
            pos: 0,
            batch_samples: (batch_sec * SAMPLE_RATE as f64) as usize,
            checked: 0,
            stats: Stats::default(),
        }
    }

    fn push(&mut self, samples: &[f32]) {
        self.stats.received += samples.len();
        self.pending.extend_from_slice(samples);
    }

    /// 말 구간을 찾아 확정할 수 있는 만큼 전사한다. 녹음이 끝났거나, 소리가 묶음 길이만큼 모였거나,
    /// 새 소리가 조금 쌓여 말이 문장 단위로 끊긴 것(무음 `SENTENCE_GAP_SEC` 이상)이 보이면 한다.
    /// 묶음 길이는 말이 끊기지 않고 이어질 때의 상한이다.
    fn advance(
        &mut self,
        finished: bool,
        speech: &mut dyn SpeechEngine,
        vad: &mut SpeechDetector,
    ) -> Result<Vec<Segment>, String> {
        let full = self.pending.len() >= self.batch_samples;
        let grown = self.pending.len() >= self.checked + CHECK_STEP_SAMPLES;
        if self.pending.is_empty() || (!finished && !full && !grown) {
            return Ok(Vec::new());
        }
        let spans = vad
            .detect(&self.pending)
            .map_err(|e| format!("VAD 실패: {}", e.message))?;
        let (guard, min_gap) = if finished || full {
            (TAIL_GUARD_SEC, 0.0)
        } else {
            (SENTENCE_GAP_SEC, SENTENCE_GAP_SEC)
        };
        let Plan { groups, keep_from } = plan(
            &spans,
            self.pending.len(),
            finished,
            self.batch_samples,
            guard,
            min_gap,
        );
        let mut segments = Vec::new();
        for group in groups {
            let audio = window::join(&self.pending, &group.spans);
            self.stats.groups += 1;
            self.stats.last_group = audio.len();
            self.stats.speech += group
                .spans
                .iter()
                .map(|&(s, e)| ((e - s).max(0.0) * SAMPLE_RATE as f64) as usize)
                .sum::<usize>();
            let transcript = speech
                .transcribe(&audio)
                .map_err(|e| format!("전사 실패: {e}"))?;
            let text = transcript.text.trim();
            if text.is_empty() {
                continue;
            }
            self.stats.texted += 1;
            let base = self.pos as f64 / SAMPLE_RATE as f64;
            let spans: Vec<_> = group
                .spans
                .iter()
                .map(|&(s, e)| (s + base, e + base))
                .collect();
            for line in lines::lines(text, &spans) {
                segments.push(Segment {
                    start_ms: (line.start_sec * 1000.0).round() as i64,
                    end_ms: (line.end_sec * 1000.0).round() as i64,
                    text: line.text,
                });
            }
        }
        self.pending.drain(..keep_from.min(self.pending.len()));
        self.pos += keep_from;
        self.checked = self.pending.len();
        Ok(segments)
    }
}

impl Stats {
    #[expect(
        clippy::print_stderr,
        reason = "숫자 요약은 표준 오류로만 낸다. 표준 출력은 약속한 JSON 줄만 쓴다"
    )]
    fn report(&self) {
        let ms = |samples: usize| samples * 1000 / SAMPLE_RATE;
        eprintln!(
            "stats received_ms={} speech_ms={} groups={} texted_groups={} last_group_ms={}",
            ms(self.received),
            ms(self.speech),
            self.groups,
            self.texted,
            ms(self.last_group)
        );
    }
}

fn decode(bytes: &[u8], encoding: Encoding) -> Vec<f32> {
    match encoding {
        Encoding::PcmS16le => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
            .collect(),
        Encoding::PcmF32le => bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect(),
    }
}

#[expect(
    clippy::print_stderr,
    reason = "진단 글은 표준 오류로만 낸다. 표준 출력은 약속한 JSON 줄만 쓴다"
)]
fn fail(message: &str) -> ExitCode {
    eprintln!("meet-transcriber: {message}");
    ExitCode::from(2)
}

// 앱과의 약속은 표준 출력의 JSON 줄이다.
fn send(response: &Response) -> std::io::Result<()> {
    let line = serde_json::to_string(response).map_err(std::io::Error::other)?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}")?;
    out.flush()
}

struct Args {
    model: PathBuf,
    vad_model: PathBuf,
    language: String,
    batch_sec: f64,
}

impl Args {
    fn parse(mut argv: impl Iterator<Item = String>) -> Result<Self, String> {
        let (mut model, mut vad_model, mut language, mut batch_sec) =
            (None, None, "ko".to_string(), 20.0);
        while let Some(flag) = argv.next() {
            let value = argv
                .next()
                .ok_or_else(|| format!("{flag} 뒤에 값이 없습니다"))?;
            match flag.as_str() {
                "--model" => model = Some(PathBuf::from(value)),
                "--vad-model" => vad_model = Some(PathBuf::from(value)),
                "--language" => language = value,
                "--batch-sec" => {
                    batch_sec = value
                        .parse()
                        .map_err(|_| "--batch-sec는 초(숫자)입니다".to_string())?
                }
                _ => return Err(format!("모르는 인자입니다: {flag}")),
            }
        }
        Ok(Self {
            model: model.ok_or("--model이 필요합니다")?,
            vad_model: vad_model.ok_or("--vad-model이 필요합니다")?,
            language,
            batch_sec,
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "_tag", rename_all = "camelCase")]
enum Request {
    #[serde(rename_all = "camelCase")]
    Audio {
        seq: i64,
        sample_rate: u32,
        channels: u32,
        encoding: Encoding,
        data_base64: String,
    },
    Finish {},
}

#[derive(Deserialize, Clone, Copy)]
enum Encoding {
    #[serde(rename = "pcm-s16le")]
    PcmS16le,
    #[serde(rename = "pcm-f32le")]
    PcmF32le,
}

#[derive(Serialize)]
#[serde(tag = "_tag", rename_all = "camelCase")]
enum Response {
    Ready { gpu: Gpu },
    Segments { seq: i64, segments: Vec<Segment> },
    Done { segments: Vec<Segment> },
}

#[derive(Serialize)]
#[serde(tag = "_tag", rename_all = "camelCase")]
enum Gpu {
    Used { device: String },
    Unavailable { reason: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Segment {
    start_ms: i64,
    end_ms: i64,
    text: String,
}
