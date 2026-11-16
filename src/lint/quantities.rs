//! 数えた文字数の照合と、測定量を個数として扱う表現の確認候補。

use std::sync::LazyLock;

use regex::Regex;

use super::item_count::parse_number;
use super::morph::TokenizedSentence;
use super::{Finding, make_span};
use crate::morphology::Morpheme;

static CHARACTER_CLAIM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:「(?P<single>[^「」『』\n]+)」|『(?P<double>[^「」『』\n]+)』)(?:という|の)(?P<count>[0-9０-９]+|[一二三四五六七八九十]+)(?:文字|字)",
    )
    .expect("valid quoted character count regex")
});

// 絵文字の連結・結合文字・異体字セレクタ等は、scalar数と見た目の字数が一致しない。
// nwiizo-coding-style: 素の漢字・仮名・英数字・空白だけに限定し、複雑な表記は判定しない;
// 対象を広げる際は見た目の文字単位と読みの字数を区別する用例を用意する。
static PLAIN_CHARACTERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\p{Han}ぁ-ゖァ-ヺーA-Za-zＡ-Ｚａ-ｚ0-9０-９ \t　]+$")
        .expect("valid plain character regex")
});

fn sentence_end(text: &str) -> bool {
    let text = text.trim_start();
    text.is_empty() || text.starts_with(['。', '！', '!'])
}

fn exact_count_claim(rest: &str) -> bool {
    // 上限・概数・部分の文字・疑問・否定を、全文字数の断定に取り違えない。
    let rest = rest.trim_start();
    sentence_end(rest)
        || ["である", "でした", "だった", "です", "だ"]
            .iter()
            .any(|ending| rest.strip_prefix(ending).is_some_and(sentence_end))
}

fn update_quote_depth(depth: &mut usize, text: &str) {
    for ch in text.chars() {
        match ch {
            '「' | '『' => *depth += 1,
            '」' | '』' => *depth = depth.saturating_sub(1),
            _ => {}
        }
    }
}

pub(super) fn quoted_character_count_findings(masked: &str, raw_lines: &[&str]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut quote_depth = 0;
    for (index, line) in masked.split('\n').enumerate() {
        let mut previous_end = 0;
        for captures in CHARACTER_CLAIM.captures_iter(line) {
            let whole = captures.get(0).expect("whole count claim");
            update_quote_depth(&mut quote_depth, &line[previous_end..whole.start()]);
            let in_quote = quote_depth > 0;
            update_quote_depth(&mut quote_depth, whole.as_str());
            previous_end = whole.end();
            if in_quote
                || !exact_count_claim(&line[whole.end()..])
                || raw_lines[index].get(whole.range()) != Some(whole.as_str())
            {
                continue;
            }
            let text = captures
                .name("single")
                .or_else(|| captures.name("double"))
                .expect("matched quote has content")
                .as_str();
            if !PLAIN_CHARACTERS.is_match(text) {
                continue;
            }
            let Some(declared) = parse_number(&captures["count"]) else {
                continue;
            };
            let with_spaces = text.chars().count();
            let without_spaces = text.chars().filter(|ch| !ch.is_whitespace()).count();
            // 空白を数える流儀と省く流儀のどちらに一致しても、指摘しない。
            if declared == with_spaces || declared == without_spaces {
                continue;
            }
            let observed = if with_spaces == without_spaces {
                format!("引用内は{with_spaces}字")
            } else {
                format!("引用内は空白を含めると{with_spaces}字、除くと{without_spaces}字")
            };
            let mut finding = Finding::new(
                index + 1,
                "quoted_character_count_mismatch",
                whole.as_str(),
                "info",
                format!(
                    "{observed}ですが、{declared}字と記述されています。数える範囲や表記を確認してください。"
                ),
            );
            finding.span = make_span(raw_lines, index + 1, whole.start(), index + 1, whole.end());
            findings.push(finding);
        }
        update_quote_depth(&mut quote_depth, &line[previous_end..]);
    }
    findings
}

#[derive(Clone, Copy)]
enum Quantity {
    Measurement,
    Force,
    /// 疲れ・機嫌など、形のない状態。
    State,
}

fn quantity_argument(tokens: &[&Morpheme], start: usize) -> Option<(Quantity, usize)> {
    let token = tokens[start];
    if token.pos(0) == "名詞" && token.pos(1) == "普通名詞" {
        match token.normalized() {
            "速度" | "速さ" | "距離" | "長さ" | "重さ" | "重量" | "質量" | "温度" | "気温" =>
            {
                return Some((Quantity::Measurement, start));
            }
            "力" => return Some((Quantity::Force, start)),
            "疲れ" | "疲労" | "機嫌" | "眠気" | "緊張" | "不安" | "退屈" => {
                return Some((Quantity::State, start));
            }
            _ => {}
        }
    }
    if token.pos(0) == "形容詞"
        && matches!(token.normalized(), "速い" | "長い" | "重い")
        && tokens
            .get(start + 1)
            .is_some_and(|next| next.pos(0) == "接尾辞" && next.surface == "さ")
    {
        return Some((Quantity::Measurement, start + 1));
    }
    None
}

fn one_item_end(tokens: &[&Morpheme], start: usize) -> Option<usize> {
    let token = tokens.get(start)?;
    if matches!(token.surface.as_str(), "一つ" | "ひとつ") {
        return Some(start + 1);
    }
    (matches!(token.surface.as_str(), "一" | "1" | "１" | "ひと")
        && tokens
            .get(start + 1)
            .is_some_and(|next| next.surface == "つ"))
    .then_some(start + 2)
}

// 「一つずつ」は状態にも使われる（不安を一つずつ解消する）ため、形のある物の助数詞だけを見る。
fn shaped_count_end(tokens: &[&Morpheme], start: usize) -> Option<usize> {
    let number = tokens.get(start)?;
    let counter = tokens.get(start + 1)?;
    if !(number.pos(1) == "数詞" || number.surface == "ひと")
        || !matches!(counter.pos(0), "接尾辞" | "名詞")
        || !matches!(
            counter.surface.as_str(),
            "枚" | "本" | "粒" | "滴" | "切れ" | "片"
        )
    {
        return None;
    }
    let end = start + 2;
    Some(if tokens.get(end).is_some_and(|t| t.surface == "ずつ") {
        end + 1
    } else {
        end
    })
}

fn quantity_predicate(tokens: &[&Morpheme], end: usize, quantity: Quantity) -> Option<usize> {
    let particle = tokens.get(end + 1)?;
    // 状態は「疲れが一枚ずつ剥がれる」のように主語でも数えられる。測定量・力は目的語だけを見る。
    let subject = matches!(quantity, Quantity::State) && particle.surface == "が";
    if particle.pos(0) != "助詞" || !(particle.surface == "を" || subject) {
        return None;
    }
    let start = match quantity {
        Quantity::Measurement => end + 2,
        Quantity::Force => one_item_end(tokens, end + 2)?,
        Quantity::State => shaped_count_end(tokens, end + 2)?,
    };
    let mut index = start;
    // 副詞を挟んだ最初の動詞だけを見る。別の項・節へは接続しない。
    while index < tokens.len().min(start + 8) {
        let token = tokens[index];
        match token.pos(0) {
            "動詞" => {
                let expected = match quantity {
                    Quantity::Measurement => Some("数える"),
                    Quantity::Force => Some("抜く"),
                    Quantity::State => None,
                };
                return expected
                    .is_none_or(|verb| token.normalized() == verb)
                    .then_some(index);
            }
            "副詞" => index += 1,
            "形容詞" if token.pos(5).starts_with("連用形") => index += 1,
            "形状詞"
                if tokens
                    .get(index + 1)
                    .is_some_and(|next| next.surface == "に") =>
            {
                index += 2;
            }
            _ => return None,
        }
    }
    None
}

fn asserted_quantity_predicate(tokens: &[&Morpheme], predicate: usize) -> bool {
    for index in predicate + 1..tokens.len() {
        let token = tokens[index];
        if matches!(token.normalized(), "ない" | "無い" | "ぬ" | "ず" | "ん") {
            return false;
        }
        match token.pos(0) {
            "助動詞" => {}
            "動詞" if token.pos(1) == "非自立可能" => {}
            // 引用・名詞化に加え、「数えはしない」「数えてはいけない」や禁止を除く。
            "助詞"
                if matches!(
                    token.surface.as_str(),
                    "と" | "って" | "の" | "は" | "も" | "な"
                ) =>
            {
                return false;
            }
            "助詞" if matches!(token.surface.as_str(), "て" | "で") => {}
            // 「数えて記録する」は対象。「数える方法」「数えている時間」は対象外。
            "名詞" | "代名詞" => {
                return tokens[index - 1].pos(0) == "助詞"
                    && matches!(tokens[index - 1].surface.as_str(), "て" | "で");
            }
            _ => break,
        }
    }
    true
}

// nwiizo-coding-style: 測定量＋数える、力＋一つ＋抜く、状態（を・が）＋形のある物の助数詞に限定し、意味全般は推論しない;
// 拡張する際は可算な語義、否定、引用、別の述語へ係る用例を対照に加える。
pub(super) fn quantity_predicate_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut bracket_depth: usize = 0;
    for sentence in tokenized {
        let tokens = sentence
            .tokens
            .iter()
            .filter(|token| {
                !token
                    .surface
                    .chars()
                    .all(|ch| ch.is_whitespace() || matches!(ch, '*' | '_'))
            })
            .collect::<Vec<_>>();
        for (index, token) in tokens.iter().enumerate() {
            match token.pos(1) {
                "括弧開" => bracket_depth += 1,
                "括弧閉" => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            if bracket_depth > 0 {
                continue;
            }
            let Some((quantity, end)) = quantity_argument(&tokens, index) else {
                continue;
            };
            let Some(predicate) = quantity_predicate(&tokens, end, quantity) else {
                continue;
            };
            if !asserted_quantity_predicate(&tokens, predicate) {
                continue;
            }
            let detail = match quantity {
                Quantity::Measurement => {
                    "速度・距離・温度などの測定量を「数える」対象にしています。測る値なのか、数える個数・回数なのかを確認してください。種類や目盛りを数える意図なら、その対象を示すと明確になります。"
                }
                Quantity::Force => {
                    "力を「一つ」抜く形になっています。筋肉の緊張を表すなら個数とは区別し、懸念や能力などを数える意図なら、その対象を確認してください。"
                }
                Quantity::State => {
                    "疲れや機嫌のような形のない状態を、枚・本・粒など形のある物の数え方で数えています。人・場面・回数のどれを一つずつ扱うのかを確認してください。意図した比喩は残せます。"
                }
            };
            findings.push(sentence.info_finding(
                raw_lines,
                token.byte_start..tokens[predicate].byte_end,
                "quantity_predicate_mismatch",
                detail,
            ));
        }
    }
    findings
}
