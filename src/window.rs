//! 아직 확정하지 않은 소리에서 이번에 전사할 말 구간 묶음과 다음에 다시 볼 자리를 정한다(순수 함수).
//! 묶는 규칙: 말 구간을 28초 안팎으로 묶고, 끝에 걸친 말은 다음에 본다.

pub const SAMPLE_RATE: usize = 16_000;
/// 모델이 한 번에 보는 길이가 30초라 28초 묶음이 가장 빠르다.
const MERGE_SEC: f64 = 28.0;
/// 묶음 안 구간 사이에 끼우는 무음.
const GAP_SEC: f64 = 0.3;
/// 소리가 묶음 길이만큼 모였을 때, 끝에서 이만큼 안에 끝나는 말은 더 이어질 수 있어 확정하지 않는다.
pub const TAIL_GUARD_SEC: f64 = 0.5;
/// 묶음 길이를 기다리지 않고 문장 단위로 확정할 때 말 뒤에 필요한 무음(말이 끊겼다고 보는 길이).
/// 말 구간 사이 간격이 이보다 짧으면 한 문장 안의 쉼으로 보고 함께 둔다.
pub const SENTENCE_GAP_SEC: f64 = 0.7;
/// whisper.cpp는 너무 짧은 입력을 거부하므로 이 길이까지 0으로 채운다.
const MIN_SAMPLES: usize = SAMPLE_RATE + SAMPLE_RATE / 10;
/// 말이 끊이지 않아 확정할 수 없어도 묶음 길이의 이 배수를 넘으면 모두 확정한다(글이 너무 늦게 나오지 않게).
const FORCE_AFTER_BATCHES: usize = 3;

pub type Span = (f64, f64);

#[derive(Debug, PartialEq)]
pub struct Group {
    pub spans: Vec<Span>,
    /// 묶음의 처음과 끝(샘플, 확정하지 않은 소리의 처음 기준)
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, PartialEq)]
pub struct Plan {
    pub groups: Vec<Group>,
    /// 이 샘플 앞은 다 썼다. 이 자리부터 다음에 다시 본다.
    pub keep_from: usize,
}

/// `spans`는 확정하지 않은 소리(`len` 샘플)에서 찾은 말 구간(초)이다.
/// `guard_sec`: 끝에서 이만큼 안에 끝나는 말은 확정하지 않는다.
/// `min_gap_sec`: 확정하는 마지막 구간과 그다음 구간 사이가 이보다 짧으면 그 구간은 다음에 본다(0이면 따지지 않는다).
pub fn plan(
    spans: &[Span],
    len: usize,
    finished: bool,
    batch_samples: usize,
    guard_sec: f64,
    min_gap_sec: f64,
) -> Plan {
    let buf_sec = len as f64 / SAMPLE_RATE as f64;
    let cutoff = if finished {
        buf_sec
    } else {
        (buf_sec - guard_sec).max(0.0)
    };
    let mut settled = spans.iter().take_while(|&&(_, e)| e <= cutoff).count();
    if !finished {
        while settled > 0
            && spans
                .get(settled)
                .is_some_and(|&(s, _)| s - spans[settled - 1].1 < min_gap_sec)
        {
            settled -= 1;
        }
    }
    let force =
        !finished && settled == 0 && len >= batch_samples.saturating_mul(FORCE_AFTER_BATCHES);
    let (taken, keep_from) = if finished || force {
        (spans, len)
    } else {
        let next = spans.get(settled).map_or(cutoff, |&(s, _)| s.min(cutoff));
        (&spans[..settled], to_samples(next).min(len))
    };
    Plan {
        groups: group(taken),
        keep_from,
    }
}

/// 묶음의 소리를 이어 붙인다. 구간 사이에는 무음을 끼우고, 너무 짧으면 0으로 채운다.
pub fn join(buf: &[f32], spans: &[Span]) -> Vec<f32> {
    let gap = to_samples(GAP_SEC);
    let mut out = Vec::new();
    for (i, &(s, e)) in spans.iter().enumerate() {
        if i > 0 {
            out.extend(std::iter::repeat_n(0.0f32, gap));
        }
        let (a, b) = (to_samples(s).min(buf.len()), to_samples(e).min(buf.len()));
        out.extend_from_slice(&buf[a..b.max(a)]);
    }
    if out.len() < MIN_SAMPLES {
        out.resize(MIN_SAMPLES, 0.0);
    }
    out
}

fn group(spans: &[Span]) -> Vec<Group> {
    let mut groups: Vec<(Vec<Span>, f64)> = Vec::new();
    for &(s, e) in spans.iter().filter(|(s, e)| e > s) {
        match groups.last_mut() {
            Some((g, len)) if *len + GAP_SEC + (e - s) <= MERGE_SEC => {
                g.push((s, e));
                *len += GAP_SEC + (e - s);
            }
            _ => groups.push((vec![(s, e)], e - s)),
        }
    }
    groups
        .into_iter()
        .map(|(spans, _)| Group {
            start: to_samples(spans[0].0),
            end: to_samples(spans[spans.len() - 1].1),
            spans,
        })
        .collect()
}

fn to_samples(sec: f64) -> usize {
    (sec.max(0.0) * SAMPLE_RATE as f64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const BATCH: usize = 20 * SAMPLE_RATE;

    #[test]
    fn holds_speech_touching_the_end_until_more_audio_comes() {
        let p = plan(
            &[(1.0, 5.0), (8.0, 19.8)],
            20 * SAMPLE_RATE,
            false,
            BATCH,
            TAIL_GUARD_SEC,
            0.0,
        );
        assert_eq!(p.groups.len(), 1);
        assert_eq!(p.groups[0].spans, vec![(1.0, 5.0)]);
        assert_eq!(p.keep_from, 8 * SAMPLE_RATE);
    }

    #[test]
    fn finishing_takes_every_span_and_all_audio() {
        let p = plan(
            &[(1.0, 5.0), (8.0, 19.8)],
            20 * SAMPLE_RATE,
            true,
            BATCH,
            TAIL_GUARD_SEC,
            0.0,
        );
        assert_eq!(p.groups[0].spans, vec![(1.0, 5.0), (8.0, 19.8)]);
        assert_eq!(p.keep_from, 20 * SAMPLE_RATE);
    }

    #[test]
    fn silence_is_dropped_up_to_the_tail_guard() {
        let p = plan(&[], 20 * SAMPLE_RATE, false, BATCH, TAIL_GUARD_SEC, 0.0);
        assert!(p.groups.is_empty());
        assert_eq!(p.keep_from, to_samples(19.5));
    }

    #[test]
    fn a_sentence_followed_by_a_long_silence_is_settled_early() {
        let p = plan(
            &[(1.0, 3.0)],
            4 * SAMPLE_RATE,
            false,
            BATCH,
            SENTENCE_GAP_SEC,
            SENTENCE_GAP_SEC,
        );
        assert_eq!(p.groups.len(), 1);
        assert_eq!(p.groups[0].spans, vec![(1.0, 3.0)]);
    }

    #[test]
    fn a_short_pause_inside_a_sentence_is_not_a_break() {
        let spans = [(1.0, 3.0), (3.4, 4.9)];
        let p = plan(
            &spans,
            5 * SAMPLE_RATE,
            false,
            BATCH,
            SENTENCE_GAP_SEC,
            SENTENCE_GAP_SEC,
        );
        assert!(p.groups.is_empty());
        assert_eq!(p.keep_from, to_samples(1.0));
    }

    #[test]
    fn endless_speech_is_forced_after_three_batches() {
        let len = 60 * SAMPLE_RATE;
        let p = plan(&[(0.0, 60.0)], len, false, BATCH, TAIL_GUARD_SEC, 0.0);
        assert_eq!(p.keep_from, len);
        assert_eq!(p.groups.len(), 1);
    }

    #[test]
    fn groups_stay_within_the_model_window() {
        let spans: Vec<Span> = (0..10)
            .map(|i| (i as f64 * 6.0, i as f64 * 6.0 + 5.0))
            .collect();
        let groups = group(&spans);
        assert!(groups.iter().all(|g| {
            let total: f64 = g.spans.iter().map(|(s, e)| e - s).sum::<f64>()
                + GAP_SEC * (g.spans.len() - 1) as f64;
            total <= MERGE_SEC
        }));
        assert_eq!(groups.iter().map(|g| g.spans.len()).sum::<usize>(), 10);
    }

    #[test]
    fn join_pads_short_audio_and_inserts_gaps() {
        let buf = vec![1.0f32; 2 * SAMPLE_RATE];
        assert_eq!(join(&buf, &[(0.0, 0.1)]).len(), MIN_SAMPLES);
        let joined = join(&buf, &[(0.0, 1.0), (1.0, 2.0)]);
        assert_eq!(joined.len(), 2 * SAMPLE_RATE + to_samples(GAP_SEC));
    }
}
