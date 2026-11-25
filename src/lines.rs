//! 한 묶음의 글을 문장 줄로 나누고 줄마다 말이 시작된 시각을 붙인다(순수 함수).
//! 왜: 전사 엔진은 whisper 세그먼트의 시각을 내보내지 않고 글만 돌려준다. 그래서 묶음(최대 28초)이
//! 한 줄로 나왔다. 글을 문장으로 나누고, 묶음 안 말소리 구간(VAD)의 누적 길이에 글자 수 비율로 시각을 놓는다.
//! 시각은 문장 안에서 말 속도가 일정하다고 본 근사값이다. 묶음의 처음과 끝, 구간 사이 무음은 실제 값을 쓴다.

use crate::window::Span;

#[derive(Debug, PartialEq)]
pub struct Line {
    pub start_sec: f64,
    pub end_sec: f64,
    pub text: String,
}

/// `spans`는 묶음의 말 구간(초, 녹음 처음 기준으로 옮긴 값). 반환하는 줄의 시작 시각은 순증가한다.
pub fn lines(text: &str, spans: &[Span]) -> Vec<Line> {
    let sentences = sentences(text);
    let spoken: f64 = spans.iter().map(|&(s, e)| e - s).sum();
    let weights: Vec<usize> = sentences.iter().map(|s| s.chars().count()).collect();
    let total: usize = weights.iter().sum();
    if sentences.is_empty() || spans.is_empty() || spoken <= 0.0 || total == 0 {
        return Vec::new();
    }
    let mut done = 0usize;
    sentences
        .into_iter()
        .zip(weights)
        .map(|(text, weight)| {
            let from = spoken * done as f64 / total as f64;
            done += weight;
            let to = spoken * done as f64 / total as f64;
            Line {
                start_sec: at(spans, from, false),
                end_sec: at(spans, to, true),
                text,
            }
        })
        .collect()
}

/// `.` `?` `!` `。` 뒤에 공백이 오거나 글이 끝나는 자리에서 자른다(`3.5`처럼 숫자 사이 점은 자르지 않는다).
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = text.trim().chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        let ends = matches!(c, '.' | '?' | '!' | '。') && chars.peek().is_none_or(|n| n.is_whitespace());
        if ends {
            push_trimmed(&mut out, &mut current);
        }
    }
    push_trimmed(&mut out, &mut current);
    out
}

fn push_trimmed(out: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    current.clear();
}

/// 말소리만 이어 붙였을 때 `offset`초 자리가 실제 녹음의 몇 초인지. 끝 시각은 구간 끝에 붙고, 시작 시각은 다음 구간 처음으로 간다.
fn at(spans: &[Span], offset: f64, is_end: bool) -> f64 {
    let mut left = offset;
    for &(s, e) in spans {
        let len = e - s;
        let inside = if is_end { left <= len } else { left < len };
        if inside {
            return s + left;
        }
        left -= len;
    }
    spans.last().map_or(0.0, |&(_, e)| e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sentences_and_keeps_decimals() {
        assert_eq!(
            sentences("녹음이 되고 있나요? 3.5배 빠릅니다. 네"),
            vec!["녹음이 되고 있나요?", "3.5배 빠릅니다.", "네"]
        );
    }

    #[test]
    fn one_group_becomes_many_lines_with_increasing_starts() {
        let text = "녹음이 되고 있나요? 녹음이 되는지 어떻게 알죠? 말을 하고 있는데 녹음이 되나요?";
        let out = lines(text, &[(10.0, 22.0)]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].start_sec, 10.0);
        assert!(out.windows(2).all(|w| w[0].start_sec < w[1].start_sec));
        assert!(out.windows(2).all(|w| w[0].end_sec <= w[1].start_sec));
        assert_eq!(out[2].end_sec, 22.0);
    }

    #[test]
    fn silence_between_spans_is_not_counted_as_speech() {
        // 앞 문장은 0~2초, 뒤 문장은 60~62초(한 시간 회의 중간처럼 큰 값)에 말했다.
        let out = lines("가가가가. 나나나나.", &[(0.0, 2.0), (60.0, 62.0)]);
        assert_eq!(out[0].start_sec, 0.0);
        assert_eq!(out[1].start_sec, 60.0);
        assert_eq!(out[1].end_sec, 62.0);
    }

    #[test]
    fn empty_text_or_spans_give_no_lines() {
        assert!(lines("  ", &[(0.0, 1.0)]).is_empty());
        assert!(lines("글", &[]).is_empty());
    }
}
