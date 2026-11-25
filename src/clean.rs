//! 잡음에서 모델이 지어낸 조각을 걷어낸다. 정상적인 말의 내용은 건드리지 않는다.

/// 한두 글자짜리 소문자 영어 조각(`o.`, `n.`, `ue`, `uo.n.` 등)을 뺀다.
///
/// 한국어 회의 전사에서 잡음 구간에 자주 끼어드는 모양이다. 대문자 약어(PM, MCP)와
/// 세 글자 이상의 낱말은 그대로 둔다. 빼낸 조각을 함께 돌려준다.
pub fn strip_noise_fragments(text: &str) -> (String, Vec<String>) {
    let (removed, kept): (Vec<&str>, Vec<&str>) = text.split_whitespace().partition(|w| is_fragment(w));
    (kept.join(" "), removed.into_iter().map(str::to_string).collect())
}

fn is_fragment(word: &str) -> bool {
    // 앞뒤 문장부호(따옴표 등)를 벗긴 뒤 본다.
    let w = word.trim_matches(|c: char| matches!(c, '"' | '\'' | '“' | '”' | '‘' | '’' | ',' | '(' | ')'));
    if w.is_empty() {
        return false;
    }
    // `o.` `n.n.` `uo.n.` 처럼 (소문자 1~2자 + 점)이 이어진 것, 또는 점 없는 소문자 1~2자
    let mut rest = w;
    let mut groups = 0;
    while !rest.is_empty() {
        let letters = rest.chars().take_while(|c| c.is_ascii_lowercase()).count();
        if letters == 0 || letters > 2 {
            return false;
        }
        rest = &rest[letters..];
        groups += 1;
        if let Some(r) = rest.strip_prefix('.') {
            rest = r;
        } else if !rest.is_empty() {
            return false;
        }
    }
    groups > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_short_lowercase_fragments() {
        let (t, n) = strip_noise_fragments("o. n. 지금은 뭐해. uo.n. 네 lb. 토크를");
        assert_eq!(t, "지금은 뭐해. 네 토크를");
        assert_eq!(n, vec!["o.", "n.", "uo.n.", "lb."]);
    }

    #[test]
    fn keeps_real_english_and_acronyms() {
        let s = "RED랑 PM이랑 MCP를 쓰는 pdf and ok";
        let (t, n) = strip_noise_fragments(s);
        // `ok`는 두 글자 소문자라 빠진다. 세 글자 이상과 대문자는 남는다.
        assert_eq!(t, "RED랑 PM이랑 MCP를 쓰는 pdf and");
        assert_eq!(n, vec!["ok"]);
    }

    #[test]
    fn keeps_hangul_with_period() {
        let (t, n) = strip_noise_fragments("그래서 그냥.");
        assert_eq!(t, "그래서 그냥.");
        assert!(n.is_empty());
    }
}
