//! 品詞列（Sudachi形態素）ベースの検出器と、文単位のtoken補助。

use crate::Error;
use crate::morphology::{Morpheme, Morphology};
use crate::text::Sentence;

use super::{Finding, Span, Suggestion, make_span};

// Sudachi品詞体系。IPADICで名詞に含まれた代名詞と形容動詞語幹（形状詞）を
// 独立品詞として持つため、内容語の範囲を揃えて列挙する。
pub(super) const CONTENT_POS: &[&str] = &["名詞", "代名詞", "形状詞", "動詞", "形容詞", "副詞"];

const TRANSITIVE_SMELL_VERBS: &[&str] = &[
    "もたらす",
    "示す",
    "意味する",
    "証明する",
    "生み出す",
    "反映する",
    "示唆する",
    "物語る",
    "浮き彫りにする",
    "後押しする",
];

#[derive(Clone, Debug)]
pub(super) struct TokenizedSentence {
    pub(super) line: usize,
    pub(super) text: String,
    pub(super) raw_text: String,
    pub(super) line_byte_start: usize,
    pub(super) tokens: Vec<Morpheme>,
}

impl TokenizedSentence {
    /// 文内のtoken byte範囲を行内のbyte範囲へ写す。
    pub(super) fn span(
        &self,
        raw_lines: &[&str],
        byte_start: usize,
        byte_end: usize,
    ) -> Option<Span> {
        make_span(
            raw_lines,
            self.line,
            self.line_byte_start + byte_start,
            self.line,
            self.line_byte_start + byte_end,
        )
    }

    /// 文内byte範囲の抜粋。raw側が範囲を切り出せない場合はmasked本文へ落とす。
    pub(super) fn excerpt(&self, byte_start: usize, byte_end: usize) -> String {
        self.raw_text
            .get(byte_start..byte_end)
            .unwrap_or(&self.text[byte_start..byte_end])
            .to_owned()
    }
}

pub(super) fn tokenize(
    split: &[Sentence],
    morphology: &Morphology,
) -> Result<Vec<TokenizedSentence>, Error> {
    split
        .iter()
        .map(|sentence| {
            Ok(TokenizedSentence {
                line: sentence.line,
                text: sentence.text.clone(),
                raw_text: sentence.raw_text.clone(),
                line_byte_start: sentence.line_byte_start,
                tokens: morphology.tokenize(&sentence.text)?,
            })
        })
        .collect()
}

pub(super) fn significant_tokens(tokens: &[Morpheme]) -> &[Morpheme] {
    let start = tokens
        .iter()
        .position(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .unwrap_or(tokens.len());
    &tokens[start..]
}

pub(super) fn punctuation_between(tokens: &[Morpheme], first: usize, second: usize) -> bool {
    tokens[first + 1..second]
        .iter()
        .any(|token| matches!(token.pos(0), "記号" | "補助記号"))
}

pub(super) fn noun_ended(tokens: &[Morpheme]) -> bool {
    tokens
        .iter()
        .rev()
        .find(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .is_some_and(|token| matches!(token.pos(0), "名詞" | "代名詞"))
}

pub(super) fn buried_list(tokens: &[Morpheme]) -> Option<(usize, usize, usize)> {
    let mut bounds = Vec::new();
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if token.surface == "、" {
            bounds.push((start, index));
            start = index + 1;
        }
    }
    bounds.push((start, tokens.len()));
    let mut run = Vec::new();
    let mut best = None;
    for (index, (start, end)) in bounds.iter().copied().enumerate() {
        if end > start && noun_ended(&tokens[start..end]) {
            run.push((start, end));
        } else {
            run.clear();
        }
        if run.len() >= 2 && index + 1 < bounds.len() {
            let items = run.len() + 1;
            if best.is_none_or(|(_, _, best_items)| items > best_items) {
                best = Some((run[0].0, bounds[index + 1].1, items));
            }
        }
    }
    best
}

pub(super) fn mora_length(tokens: &[Morpheme]) -> usize {
    tokens
        .iter()
        .map(|token| {
            token
                .reading()
                .chars()
                .filter(|ch| {
                    !matches!(
                        ch,
                        'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ャ' | 'ュ' | 'ョ' | 'ヮ'
                    )
                })
                .count()
        })
        .sum()
}

pub(super) fn translationese_morph_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        for (index, token) in sentence.tokens.iter().enumerate() {
            let Some(particle) = sentence.tokens.get(index + 1) else {
                continue;
            };
            let Some(verb) = sentence.tokens.get(index + 2) else {
                continue;
            };
            // 2026-08-18の技術書翻訳21件の正解ラベルに基づき対象を絞る:
            // 「は」型(ことはできない)は否定の対比として自然、使役型
            // (させることができる)は縮約すると受身と紛れるため対象外。
            let causative = index > 0
                && matches!(
                    sentence.tokens[index - 1].dictionary_form(),
                    "せる" | "させる"
                );
            if token.surface == "こと"
                && token.pos(0) == "名詞"
                && particle.pos(0) == "助詞"
                && particle.surface == "が"
                && !causative
                && verb.pos(0) == "動詞"
                && verb.surface.starts_with("でき")
            {
                let start = sentence.tokens[index.saturating_sub(4)].byte_start;
                let mut finding = Finding::new(
                    sentence.line,
                    "translationese_morph",
                    sentence.excerpt(start, verb.byte_end),
                    "info",
                    "品詞列マッチ: 名詞/動詞+こと+が/は+できる型の翻訳調構文",
                );
                finding.span = sentence.span(raw_lines, start, verb.byte_end);
                finding.suggestion = suru_koto_ga_suggestion(sentence, raw_lines, index, particle);
                findings.push(finding);
            }
        }
    }
    findings
}

/// 機械的に安全な唯一の縮約: 「〜することができる」→「〜できる」。
/// 直前が動詞「する」で助詞が「が」の場合だけ、「することが」の削除候補を出す。
/// raw行のpreimageが一致しないときは出さない。
fn suru_koto_ga_suggestion(
    sentence: &TokenizedSentence,
    raw_lines: &[&str],
    koto_index: usize,
    particle: &Morpheme,
) -> Option<Suggestion> {
    if koto_index == 0 || particle.surface != "が" {
        return None;
    }
    let suru = sentence.tokens.get(koto_index - 1)?;
    if suru.pos(0) != "動詞" || suru.dictionary_form() != "する" {
        return None;
    }
    let koto = &sentence.tokens[koto_index];
    let expected = format!("{}{}{}", suru.surface, koto.surface, particle.surface);
    let line_start = sentence.line_byte_start + suru.byte_start;
    let line_end = sentence.line_byte_start + particle.byte_end;
    let matches_raw = raw_lines
        .get(sentence.line - 1)
        .and_then(|raw_line| raw_line.get(line_start..line_end))
        .is_some_and(|slice| slice == expected);
    if !matches_raw {
        return None;
    }
    Some(Suggestion {
        span: sentence.span(raw_lines, suru.byte_start, particle.byte_end)?,
        preimage: expected,
        replacement: String::new(),
    })
}

pub(super) fn inanimate_morph_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        let mut skip_until = None;
        for index in 0..sentence.tokens.len() {
            if skip_until.is_some_and(|skip| index <= skip) {
                continue;
            }
            let token = &sentence.tokens[index];
            let mut subject_end = index;
            let mut abstract_subject =
                matches!(token.surface.as_str(), "これ" | "それ" | "あれ" | "それら")
                    || (token.pos(0) == "名詞"
                        && matches!(token.surface.as_str(), "こと" | "事実"));
            if !abstract_subject && let Some(next) = sentence.tokens.get(index + 1) {
                let phrase = format!("{}{}", token.surface, next.surface);
                if matches!(phrase.as_str(), "この事実" | "そのこと") {
                    abstract_subject = true;
                    subject_end = index + 1;
                }
            }
            if !abstract_subject {
                continue;
            }
            skip_until = Some(subject_end);
            let Some(particle) = sentence.tokens.get(subject_end + 1) else {
                continue;
            };
            if particle.pos(0) != "助詞" || !matches!(particle.surface.as_str(), "が" | "は") {
                continue;
            }
            let verb = sentence.tokens[subject_end + 2..].iter().find(|candidate| {
                candidate.pos(0) == "動詞"
                    && TRANSITIVE_SMELL_VERBS.contains(&candidate.dictionary_form())
            });
            let Some(verb) = verb else {
                continue;
            };
            let byte_start = sentence.tokens[index.saturating_sub(3)].byte_start;
            let subject = sentence.tokens[index..=subject_end]
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<String>();
            let mut finding = Finding::new(
                sentence.line,
                "inanimate_subject_morph",
                sentence.excerpt(byte_start, verb.byte_end),
                "info",
                format!(
                    "品詞列マッチ: 抽象主語「{subject}」+ {} + 他動詞的述語「{}」（英語統語の直訳調の疑い）",
                    particle.surface,
                    verb.dictionary_form()
                ),
            );
            finding.span = sentence.span(raw_lines, byte_start, verb.byte_end);
            findings.push(finding);
        }
    }
    findings
}
