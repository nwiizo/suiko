//! 型の族: 否定した側と並べて言い切る対比（`negated_contrast`）。
//! 文書全体の密度の要約は`patterns::antithesis_findings`（`antithesis_repetition`）が持つ。

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use super::morph::{TokenizedSentence, noun_ended};
use super::{Finding, make_span};

static ADDITIVE_CONTRAST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^、?[^。]{1,40}?でも(?:ある|あります|あった|ありました)")
        .expect("valid additive-contrast regex")
});

/// 文末の打ち消し（「Aではない。」）。次の文が肯定で言い切れば「Aではない。Bだ。」の対比になる。
const NEGATED_COPULA_ENDINGS: &[&str] = &[
    "ではない",
    "ではないです",
    "ではありません",
    "じゃない",
    "じゃありません",
];

/// 次の文が、接続詞で始まらず、断定（だ・です・である）か体言止めで言い切っているか。
fn affirms(sentence: &TokenizedSentence) -> bool {
    let content = sentence
        .tokens
        .iter()
        .filter(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .collect::<Vec<_>>();
    let (Some(first), Some(last)) = (content.first(), content.last()) else {
        return false;
    };
    if first.pos(0) == "接続詞" {
        return false;
    }
    let copula = last.pos(0) == "助動詞" && matches!(last.pos(4), "助動詞-ダ" | "助動詞-デス")
        || last.pos(0) == "動詞"
            && last.dictionary_form() == "ある"
            && content.len() >= 2
            && content[content.len() - 2].surface == "で";
    copula || noun_ended(&sentence.tokens)
}

/// 「ではなく」「ではない」の前が対比でない形か。付け足し（だけではなく）、二重否定
/// （不可能ではない、少なくない）、部分否定（必ずしも〜ではない）は、否定を外すと意味が変わる。
fn not_a_contrast(sentence: &TokenizedSentence, start: usize) -> bool {
    if sentence.text[..start].contains("必ずしも") {
        return true;
    }
    let before = sentence
        .tokens
        .iter()
        .filter(|token| {
            token.byte_end <= start && !matches!(token.pos(0), "記号" | "補助記号" | "空白")
        })
        .collect::<Vec<_>>();
    let Some(head) = before.last() else {
        return false;
    };
    let negative_prefix = |surface: &str| surface.starts_with(['不', '無', '非', '未']);
    matches!(head.surface.as_str(), "だけ" | "のみ" | "ばかり")
        || head.surface.ends_with("ない")
        || negative_prefix(&head.surface)
        || before.len() >= 2
            && before[before.len() - 2].pos(0) == "接頭辞"
            && negative_prefix(&before[before.len() - 2].surface)
}

const DETAIL: &str = "否定した側と並べて言い切る対比（「AではなくB」「Aではない。Bだ。」）です。否定を外しても主張が変わらないなら、Bだけを肯定文で書けます。読み手の誤解を正す、選ばなかった案を示すなど、否定した側に意味がある対比は残してください。";

/// Bの側として範囲に含める上限（字数）。
const CONTRAST_CLAUSE_MAX_CHARS: usize = 30;

/// 「ではなく」の「なく」が状態の変化（ではなくなる、ではなくなった）の一部か。
fn state_change(sentence: &TokenizedSentence, negation: usize) -> bool {
    sentence.tokens.iter().enumerate().any(|(position, token)| {
        token.byte_start == negation
            && (token.dictionary_form() == "なくなる"
                || token.surface == "なく"
                    && sentence.tokens.get(position + 1).is_some_and(|next| {
                        next.dictionary_form() == "なる" && next.byte_start == token.byte_end
                    }))
    })
}

/// 一文の中の「AではなくB」の範囲（「ではなく」からBの節の終わりまで）。
/// 状態の変化、譲歩（ではなくても）、付け足し（ではなく、Bでもある）は対比として扱わない。
fn in_sentence_contrast(
    sentence: &TokenizedSentence,
    start: usize,
    marker: &str,
) -> Option<Range<usize>> {
    let negation = start + marker.len() - "なく".len();
    let rest = &sentence.text[start + marker.len()..];
    if state_change(sentence, negation)
        || not_a_contrast(sentence, start)
        || rest.starts_with("ても")
        || rest.starts_with("とも")
        || ADDITIVE_CONTRAST.is_match(rest)
    {
        return None;
    }
    let lead = ["て、", "、", "て"]
        .iter()
        .find(|lead| rest.starts_with(**lead))
        .map_or(0, |lead| lead.len());
    let after = &rest[lead..];
    let clause = after.find('、').unwrap_or(after.len()).min(
        after
            .char_indices()
            .nth(CONTRAST_CLAUSE_MAX_CHARS)
            .map_or(after.len(), |(byte, _)| byte),
    );
    if after[..clause].trim().is_empty() {
        return None;
    }
    Some(start..start + marker.len() + lead + clause)
}

/// 続く2文の「Aではない。Bだ。」。範囲は打ち消しから次の文の終わりまで。
/// 問いかけ（〜ではない？）と、段落をまたぐ文、接続詞で始まる文、言い切らない文は除く。
fn two_sentence_contrast(
    sentence: &TokenizedSentence,
    next: &TokenizedSentence,
    raw_lines: &[&str],
) -> Option<Finding> {
    let trimmed = sentence.text.trim_end();
    let ending = NEGATED_COPULA_ENDINGS
        .iter()
        .find(|ending| trimmed.ends_with(**ending))?;
    let start = trimmed.len() - ending.len();
    let question = matches!(sentence.end_mark, Some('？' | '?'));
    if question
        || next.line > sentence.line + 1
        || !affirms(next)
        || not_a_contrast(sentence, start)
    {
        return None;
    }
    let excerpt = format!(
        "{}{}{}",
        sentence.excerpt(start, trimmed.len()),
        sentence.end_mark.map(String::from).unwrap_or_default(),
        next.raw_text
    );
    let mut finding = Finding::new(sentence.line, "negated_contrast", excerpt, "info", DETAIL);
    finding.span = make_span(
        raw_lines,
        sentence.line,
        sentence.line_byte_start + start,
        next.line,
        next.line_byte_start + next.text.len(),
    );
    let mut lines = vec![sentence.line, next.line];
    lines.dedup();
    finding.related_lines = Some(lines);
    Some(finding)
}

/// 「AではなくB」「Aではない。Bだ。」を箇所ごとに示す。
pub(super) fn negated_contrast_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (index, sentence) in tokenized.iter().enumerate() {
        let text = sentence.text.as_str();
        findings.extend(
            text.match_indices("ではなく")
                .chain(text.match_indices("じゃなく"))
                .filter_map(|(start, marker)| in_sentence_contrast(sentence, start, marker))
                .map(|range| sentence.info_finding(raw_lines, range, "negated_contrast", DETAIL)),
        );
        if let Some(next) = tokenized.get(index + 1) {
            findings.extend(two_sentence_contrast(sentence, next, raw_lines));
        }
    }
    findings
}
