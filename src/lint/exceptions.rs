//! 全体を言い切った直後の文が、反対の状態を「だけ」で例外として足す形を指す。

use super::Finding;
use super::morph::TokenizedSentence;
use crate::morphology::Morpheme;

/// 状態の反対語。「全部閉まっている。ただ、右の扉だけが開いていた」を照合する。
const OPPOSITES: &[(&str, &str)] = &[
    ("閉まる", "開く"),
    ("閉じる", "開く"),
    ("閉める", "開ける"),
    ("消える", "点く"),
    ("消える", "付く"),
    ("止まる", "動く"),
    ("揃う", "欠ける"),
    ("埋まる", "空く"),
    ("片付く", "散らかる"),
    ("乾く", "濡れる"),
    ("眠る", "起きる"),
    ("寝る", "起きる"),
];

const EXCEPTION_LEADS: &[&str] = &[
    "ただ",
    "ただし",
    "でも",
    "だが",
    "しかし",
    "けれど",
    "けれども",
    "ところが",
];

fn negated_after(tokens: &[Morpheme], verb: usize) -> bool {
    tokens[verb + 1..]
        .iter()
        .take_while(|token| {
            token.pos(0) == "助動詞"
                || (token.pos(0) == "動詞" && token.pos(1) == "非自立可能")
                || (token.pos(1) == "接続助詞" && matches!(token.surface.as_str(), "て" | "で"))
        })
        .any(|token| matches!(token.normalized(), "ない" | "無い" | "ぬ" | "ず" | "ん"))
}

/// 全称の語と、それに続く肯定の動詞。
fn universal_state(tokens: &[Morpheme]) -> Option<(usize, &str)> {
    let universal = tokens.iter().enumerate().position(|(index, token)| {
        (matches!(token.pos(0), "名詞" | "代名詞")
            && matches!(
                token.normalized(),
                "全部" | "全て" | "全員" | "皆" | "みんな"
            ))
            || (matches!(token.surface.as_str(), "どれ" | "いずれ")
                && tokens
                    .get(index + 1)
                    .is_some_and(|next| next.surface == "も"))
    })?;
    let verb = (universal + 1..tokens.len())
        .find(|&index| tokens[index].pos(0) == "動詞" && tokens[index].pos(1) != "非自立可能")?;
    // 「閉まっているわけではない」のように、文末で全称を打ち消す形も除く。
    let negated = tokens[verb + 1..]
        .iter()
        .any(|token| matches!(token.normalized(), "ない" | "無い" | "ぬ" | "ず" | "ん"));
    (!negated).then_some((universal, tokens[verb].normalized()))
}

/// 「右の扉だけが」「窓際の席だけは」のように、「だけ」が主語を限る形。
fn restricted_subject(tokens: &[Morpheme]) -> bool {
    tokens.windows(2).any(|pair| {
        pair[0].surface == "だけ"
            && pair[0].pos(1) == "副助詞"
            && pair[1].pos(0) == "助詞"
            && matches!(pair[1].surface.as_str(), "が" | "は")
    })
}

/// 例外の文で、全称の述語を否定するか、反対の状態を述べる動詞の位置。
fn contradicting_state(tokens: &[Morpheme], state: &str) -> Option<usize> {
    if !tokens
        .iter()
        .any(|token| token.surface == "だけ" && token.pos(1) == "副助詞")
    {
        return None;
    }
    tokens.iter().enumerate().position(|(index, token)| {
        if token.pos(0) != "動詞" {
            return false;
        }
        let verb = token.normalized();
        if verb == state {
            return negated_after(tokens, index);
        }
        OPPOSITES.iter().any(|&(left, right)| {
            (left == state && right == verb) || (right == state && left == verb)
        }) && !negated_after(tokens, index)
    })
}

// nwiizo-coding-style: 隣接する2文と、反対語の表に載った状態だけを扱う。2文目は逆接の接続詞で始まるか、「だけが・だけは」で主語を限る例外に限る;
// 「全員来た。ただ、田中だけ遅れた」のような別の述語の例外は矛盾と見なさない。
pub(super) fn findings(tokenized: &[TokenizedSentence], raw_lines: &[&str]) -> Vec<Finding> {
    // 括弧の内側が複数の文にまたがる場合も、引用・台詞として除く。
    let mut depth: usize = 0;
    let quoted = tokenized
        .iter()
        .map(|sentence| {
            let mut inside = depth > 0;
            for token in &sentence.tokens {
                match token.pos(1) {
                    "括弧開" => {
                        depth += 1;
                        inside = true;
                    }
                    "括弧閉" => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            inside || !crate::markup::quotation_ranges(&sentence.text).is_empty()
        })
        .collect::<Vec<_>>();
    let mut findings = Vec::new();
    for (index, pair) in tokenized.windows(2).enumerate() {
        let (whole, exception) = (&pair[0], &pair[1]);
        // 接続詞のない文は、「だけが」「だけは」で主語を限る形に限る（「右の扉だけ、鍵が開いていた」は除く）。
        let led = EXCEPTION_LEADS
            .iter()
            .any(|lead| exception.text.trim_start().starts_with(lead));
        if exception.line > whole.line + 1
            || quoted[index]
            || quoted[index + 1]
            || !(led || restricted_subject(&exception.tokens))
        {
            continue;
        }
        let Some((_, state)) = universal_state(&whole.tokens) else {
            continue;
        };
        let Some(verb) = contradicting_state(&exception.tokens, state) else {
            continue;
        };
        let start = exception
            .tokens
            .iter()
            .find(|token| token.pos(0) != "補助記号" && !token.surface.trim().is_empty())
            .map_or(0, |token| token.byte_start);
        let mut finding = exception.info_finding(
            raw_lines,
            start..exception.tokens[verb].byte_end,
            "universal_exception_conflict",
            "前の文で全体の状態を言い切った後に、反対の状態の例外を「だけ」で足しています。前の文を「ほとんど」等に限定するか、例外を先に述べるかを確認してください。どちらが事実かを推測で決めないでください。",
        );
        if whole.line != exception.line {
            finding.related_lines = Some(vec![whole.line]);
        }
        findings.push(finding);
    }
    findings
}
