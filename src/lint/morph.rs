//! 文単位のtoken列（Sudachi形態素）と、規則が共有するtoken補助。
//! 規則は別のモジュールにある。読点は`commas`、文の構造（埋もれた並列・長い連体修飾）は`clauses`、
//! たとえの語は`abstract_metaphor`・`technical_jargon`・`sensory_terms`、否定対比は`contrast`、
//! 同じ型の反復は`repetition`、文末は`sentence_endings`、そのほかの言い回しは`phrasing`。

use crate::Error;
use crate::morphology::{Morpheme, Morphology};
use crate::text::Sentence;

use super::{Finding, Span, make_span};

// Sudachi品詞体系。IPADICで名詞に含まれた代名詞と形容動詞語幹（形状詞）を
// 独立品詞として持つため、内容語の範囲を揃えて列挙する。
pub(super) const CONTENT_POS: &[&str] = &["名詞", "代名詞", "形状詞", "動詞", "形容詞", "副詞"];

#[derive(Clone, Debug)]
pub(super) struct TokenizedSentence {
    pub(super) line: usize,
    pub(super) text: String,
    pub(super) raw_text: String,
    pub(super) end_mark: Option<char>,
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

    pub(super) fn info_finding(
        &self,
        raw_lines: &[&str],
        bytes: std::ops::Range<usize>,
        category: &str,
        detail: impl Into<String>,
    ) -> Finding {
        let mut finding = Finding::new(
            self.line,
            category,
            self.excerpt(bytes.start, bytes.end),
            "info",
            detail,
        );
        finding.span = self.span(raw_lines, bytes.start, bytes.end);
        finding
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
                end_mark: sentence.end_mark,
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

pub(super) fn token_positions(
    tokenized: &[TokenizedSentence],
) -> impl Iterator<Item = (&TokenizedSentence, usize, &Morpheme)> {
    tokenized.iter().flat_map(|sentence| {
        sentence
            .tokens
            .iter()
            .enumerate()
            .map(move |(index, token)| (sentence, index, token))
    })
}

/// 直後の否定・引用・連体修飾を、対象の動作の断定として扱わない。
pub(super) fn affirmative_predicate(tokens: &[&Morpheme], predicate: usize) -> bool {
    for token in &tokens[predicate + 1..] {
        if matches!(token.normalized(), "ない" | "無い" | "ぬ" | "ず" | "ん") {
            return false;
        }
        match token.pos(0) {
            "名詞" | "代名詞" => return false,
            "助詞" if matches!(token.surface.as_str(), "の" | "と" | "って") => return false,
            "助動詞" => {}
            "動詞" if token.pos(1) == "非自立可能" => {}
            "助詞" if matches!(token.surface.as_str(), "て" | "で") => {}
            _ => break,
        }
    }
    true
}

pub(super) fn contains_any(tokens: &[Morpheme], candidates: &[&str]) -> bool {
    position_any(tokens, candidates).is_some()
}

pub(super) fn position_any(tokens: &[Morpheme], candidates: &[&str]) -> Option<usize> {
    tokens.iter().position(|token| {
        candidates.contains(&token.dictionary_form())
            || candidates.contains(&token.surface.as_str())
    })
}
