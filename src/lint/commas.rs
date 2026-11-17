//! 区切りの族: 短い名詞句＋助詞の直後の読点（`short_topic_comma`・`short_subject_comma`・`short_object_comma`）。

use crate::morphology::Morpheme;

use super::Finding;
use super::morph::{CONTENT_POS, TokenizedSentence};

/// 読点の前の名詞句の上限（空白を除く字数）。
const SHORT_PHRASE_MAX_CHARS: usize = 15;

/// 接続の働きをする句。読点は文と文のつながりを示す。
const CONNECTIVE_PHRASES: &[&str] = &["中でも", "なかでも", "一方では", "他方では"];

/// 読点の前の、短い名詞句＋助詞。
struct ShortPhrase {
    category: &'static str,
    detail: &'static str,
    /// 名詞句に続く助詞（「は」「には」「が」など）。同じ助詞の句が並ぶ文を見分けるために持つ。
    particle: String,
}

/// 読点の前の句が、短い名詞句＋助詞かを調べる。
/// 助詞は「は」「も」（格助詞に続く「には」「では」なども）、「が」、「を」。
fn short_phrase(phrase: &[Morpheme]) -> Option<ShortPhrase> {
    let (last, rest) = phrase.split_last()?;
    let case_before = rest.last().is_some_and(|token| {
        token.pos(0) == "助詞"
            && matches!(token.pos(1), "格助詞" | "副助詞")
            && matches!(
                token.surface.as_str(),
                "に" | "で" | "から" | "と" | "へ" | "まで" | "より"
            )
    });
    let (category, detail, particles) = match (last.surface.as_str(), last.pos(1)) {
        ("は" | "も", "係助詞") => (
            "short_topic_comma",
            "短い主題の直後に読点があります。区切りが読みやすさや意図した間に必要か確認してください。必要な読点は残せます。",
            1 + usize::from(case_before),
        ),
        ("が", "格助詞") => (
            "short_subject_comma",
            "短い名詞句＋「が」の直後に読点があります。主語と述語のあいだに間を置く理由があるか確認してください。掛かり先を分けるための読点は残せます。",
            1,
        ),
        ("を", "格助詞") => (
            "short_object_comma",
            "短い名詞句＋「を」の直後に読点があります。後続の述語とのつながりを確認してください。強調や読みやすさに必要な読点は残せます。",
            1,
        ),
        _ => return None,
    };
    let noun_end = phrase.len() - particles;
    let nouns = &phrase[..noun_end];
    let noun_like = |token: &Morpheme| {
        matches!(
            token.pos(0),
            "名詞" | "代名詞" | "連体詞" | "接頭辞" | "接尾辞" | "空白"
        )
    };
    // 名詞をつなぐ「の」だけを許し、句の始めと終わりには置かない。
    let joined = nouns.iter().enumerate().all(|(index, token)| {
        noun_like(token)
            || token.surface == "の"
                && token.pos(1) == "格助詞"
                && index > 0
                && index + 1 < nouns.len()
    });
    let content = nouns
        .iter()
        .filter(|token| token.pos(0) != "空白")
        .collect::<Vec<_>>();
    let head = content.last()?;
    let chars = content
        .iter()
        .map(|token| token.surface.chars().count())
        .sum::<usize>();
    let text = phrase
        .iter()
        .map(|token| token.surface.as_str())
        .collect::<String>();
    // 指示語・人称だけの句、副詞にもなる名詞（今回・現在）、接続の句は、文のつながりを示す。
    let pronoun_only = content
        .iter()
        .all(|token| matches!(token.pos(0), "代名詞" | "接尾辞"));
    // 副詞可能の名詞は主題（今回は）のときだけ除く。目的語・主語（結果を、）は名詞として扱う。
    let adverbial = category == "short_topic_comma" && head.pos(2) == "副詞可能";
    let connective = CONNECTIVE_PHRASES.contains(&text.trim())
        || head.surface == "的"
            && case_before
            && rest.last().is_some_and(|token| token.surface == "に");
    (joined && chars <= SHORT_PHRASE_MAX_CHARS && !pronoun_only && !adverbial && !connective).then(
        || ShortPhrase {
            category,
            detail,
            particle: phrase[noun_end..]
                .iter()
                .map(|token| token.surface.as_str())
                .collect(),
        },
    )
}

fn bracket_depth(text: &str) -> i32 {
    text.chars()
        .map(|ch| match ch {
            '「' | '『' | '（' | '(' | '【' | '［' | '“' => 1,
            '」' | '』' | '）' | ')' | '】' | '］' | '”' => -1,
            _ => 0,
        })
        .sum()
}

/// 短い名詞句＋助詞の直後の読点を、文の中のすべての読点について示す。
/// 並べた名詞の最後、同じ助詞の句の並び、括弧の中、後ろに別の主語や連体修飾が続く読点は除く。
pub(super) fn short_phrase_comma_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        let tokens = &sentence.tokens;
        let commas = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.surface == "、")
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if commas.is_empty() {
            continue;
        }
        // 唯一の読点を外すことで、読点のない60字以上の文を作らない。
        if sentence.text.matches(['、', '，', ',']).count() == 1
            && crate::text::reading_length(&sentence.text) > 60
        {
            continue;
        }
        // 読点で区切った句。先頭の記号と空白は句に含めない。
        let segments = commas
            .iter()
            .enumerate()
            .map(|(k, &comma_index)| {
                let start = if k == 0 { 0 } else { commas[k - 1] + 1 };
                let tokens = &tokens[start..comma_index];
                let leading = tokens
                    .iter()
                    .take_while(|token| matches!(token.pos(0), "空白" | "記号" | "補助記号"))
                    .count();
                &tokens[leading..]
            })
            .collect::<Vec<_>>();
        let candidates = segments
            .iter()
            .map(|segment| short_phrase(segment))
            .collect::<Vec<_>>();
        for (k, &comma_index) in commas.iter().enumerate() {
            let Some(candidate) = &candidates[k] else {
                continue;
            };
            let comma = &tokens[comma_index];
            let phrase_start = segments[k][0].byte_start;
            // 並べた名詞の最後（A、B、Cは、）。
            let listed = k > 0
                && !segments[k - 1].is_empty()
                && segments[k - 1]
                    .iter()
                    .all(|token| matches!(token.pos(0), "名詞" | "接頭辞" | "接尾辞" | "空白"));
            // 同じ助詞の句の並び（Aも、Bも、）。
            let repeated = candidates
                .iter()
                .flatten()
                .filter(|other| other.particle == candidate.particle)
                .count()
                > 1;
            let raw_line = raw_lines
                .get(sentence.line - 1)
                .copied()
                .unwrap_or_default();
            let raw_phrase = raw_line.get(
                sentence.line_byte_start + phrase_start..sentence.line_byte_start + comma.byte_end,
            );
            if listed
                || repeated
                || bracket_depth(&sentence.text[..comma.byte_start]) > 0
                || !tokens[comma_index + 1..]
                    .iter()
                    .any(|token| CONTENT_POS.contains(&token.pos(0)))
                || raw_phrase != Some(&sentence.text[phrase_start..comma.byte_end])
                || comma_separates_clause(&tokens[comma_index + 1..])
            {
                continue;
            }
            // 読点を外すと、記号で始まる太字が前の文字へ接して表示されなくなる。
            if raw_line
                .get(sentence.line_byte_start + comma.byte_end..)
                .and_then(|tail| tail.strip_prefix("**"))
                .is_some_and(|inner| crate::markup::is_punctuation(inner.chars().next()))
            {
                continue;
            }
            let mut finding = sentence.info_finding(
                raw_lines,
                comma.byte_start..comma.byte_end,
                candidate.category,
                candidate.detail,
            );
            finding.excerpt = sentence.excerpt(phrase_start, comma.byte_end);
            findings.push(finding);
        }
    }
    findings
}

/// 短い句でも、後ろに別の主語や連体修飾があれば読点が掛かり先を分けうる。
/// 係り受けは確定できないため、この形には削除を促す候補を出さない。
/// 次の読点までを見て、括弧や記号は飛ばす（「閣議決定された「…」」の連体修飾を見落とさない）。
fn comma_separates_clause(tokens: &[Morpheme]) -> bool {
    let clause = tokens
        .iter()
        .take_while(|token| !matches!(token.surface.as_str(), "、" | "，" | "," | "。"))
        .filter(|token| !matches!(token.pos(0), "補助記号" | "記号" | "空白"))
        .collect::<Vec<_>>();
    clause.iter().any(|token| {
        token.pos(0) == "助詞"
            && matches!(
                (token.surface.as_str(), token.pos(1)),
                ("が", "格助詞") | ("は", "係助詞")
            )
    }) || clause.windows(2).any(|pair| {
        // 活用する接尾辞（嫌がる・らしい）も連体修飾を作る。
        matches!(pair[0].pos(0), "動詞" | "形容詞" | "助動詞" | "接尾辞")
            && pair[0].pos(5).starts_with("連体形")
            && pair[1].pos(0) == "名詞"
    })
}
