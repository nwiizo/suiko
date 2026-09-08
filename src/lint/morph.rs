//! 品詞列（Sudachi形態素）ベースの検出器と、文単位のtoken補助。

use std::collections::BTreeMap;

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

const ABSTRACT_METAPHOR_NOUNS: &[&str] = &[
    "地図",
    "羅針盤",
    "\u{5951}\u{7d04}",
    "道標",
    "土台",
    "架け橋",
];

const ABSTRACT_CONTEXT_NOUNS: &[&str] = &[
    "実装",
    "判断",
    "設計",
    "仕様",
    "方針",
    "計画",
    "戦略",
    "議論",
    "思考",
    "理解",
    "運用",
    "開発",
    "組織",
    "事業",
    "変革",
    "成長",
    "課題",
    "解決",
    "意思決定",
];

const SELF_LABEL_EVALUATIONS: &[&str] = &["必要", "重要", "大切", "面白い", "興味深い"];

const TECHNICAL_STATUS_NOUNS: &[&str] = &[
    "テスト",
    "テスト結果",
    "検査結果",
    "CI",
    "ビルド",
    "パイプライン",
];

const DISPLAY_NOUNS: &[&str] = &[
    "表示",
    "アイコン",
    "ランプ",
    "バッジ",
    "画面",
    "背景",
    "文字",
    "色",
    "線",
];

const SOFTWARE_NOUNS: &[&str] = &[
    "コード",
    "機能",
    "変更",
    "修正",
    "ソフトウェア",
    "API",
    "実装",
];

const PHYSICAL_SHIPMENT_NOUNS: &[&str] = &[
    "工場", "倉庫", "在庫", "配送", "端末", "機器", "製品", "検品",
];

const ABSTRACT_TRANSPORT_SUBJECTS: &[&str] = &["仕様", "設計", "仕組み", "指標", "議論"];

const ABSTRACT_TRANSPORT_OBJECTS: &[&str] = &["意図", "判断", "理解", "実装", "成果"];

const ABSTRACT_EFFECT_SUBJECTS: &[&str] = &["複雑", "品質", "生産性", "リスク"];

const QUANTITY_NOUN_ENDINGS: &[&str] = &["量", "数"];

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

fn token_positions(
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

struct AggregateHit {
    line: usize,
    excerpt: String,
    span: Option<Span>,
    related_lines: Vec<usize>,
}

fn aggregate_pattern_finding(
    hits: Vec<AggregateHit>,
    min_hits: usize,
    category: &str,
    detail: impl FnOnce(usize, &str) -> String,
) -> Vec<Finding> {
    if hits.len() < min_hits {
        return Vec::new();
    }
    let count = hits.len();
    let mut related_lines = hits
        .iter()
        .flat_map(|hit| hit.related_lines.iter().copied())
        .collect::<Vec<_>>();
    related_lines.sort_unstable();
    related_lines.dedup();
    let related = related_lines
        .iter()
        .map(|line| format!("L{line}"))
        .collect::<Vec<_>>()
        .join(", ");
    let first = hits.into_iter().next().expect("hits is not empty");
    let mut finding = Finding::new(
        first.line,
        category,
        first.excerpt,
        "info",
        detail(count, &related),
    );
    finding.related_lines = Some(related_lines);
    finding.span = first.span;
    vec![finding]
}

pub(super) fn self_labeling_repetition_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let hits = tokenized
        .iter()
        .filter_map(|sentence| {
            let (byte_start, byte_end) = self_labeling_span(&sentence.tokens)?;
            Some(AggregateHit {
                line: sentence.line,
                excerpt: sentence.excerpt(byte_start, byte_end),
                span: sentence.span(raw_lines, byte_start, byte_end),
                related_lines: vec![sentence.line],
            })
        })
        .collect::<Vec<_>>();
    aggregate_pattern_finding(hits, 3, "self_labeling_repetition", |count, related| {
        format!(
            "評価語を含む「〜のは」型の主題提示が{}回ある（閾値3回以上）。重要点の提示は談話を導く働きもあるため、使用自体ではなく文書内の反復だけを確認する実験的検出。対応箇所: {related}",
            count
        )
    })
}

fn self_labeling_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let tokens = significant_tokens(tokens);
    let start = tokens.first()?.byte_start;

    for index in 1..tokens.len().saturating_sub(1) {
        if index > 7 {
            break;
        }
        if tokens[index].surface != "の" || tokens[index + 1].surface != "は" {
            continue;
        }
        let prefix = &tokens[..index];
        let evaluation = prefix
            .iter()
            .rev()
            .take(4)
            .any(|token| SELF_LABEL_EVALUATIONS.contains(&token.dictionary_form()));
        let avoid_desire = prefix
            .iter()
            .any(|token| token.dictionary_form() == "避ける")
            && prefix.iter().any(|token| token.dictionary_form() == "たい");
        if evaluation || avoid_desire {
            return Some((start, tokens[index + 1].byte_end));
        }
    }

    None
}

pub(super) fn explanation_preview_findings(
    tokenized: &[TokenizedSentence],
    raw: &str,
) -> Vec<Finding> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    // コード内の「#」や参考文献を節の区切りとして扱わない。
    let structural = crate::text::mask_markdown_structure_preserving_headings(raw);
    let heading_lines = structural
        .lines()
        .enumerate()
        .filter_map(|(index, line)| crate::text::is_heading(line).then_some(index + 1))
        .collect::<Vec<_>>();
    let mut sections = BTreeMap::<usize, Vec<AggregateHit>>::new();
    for sentence in tokenized {
        let raw_line = raw_lines[sentence.line - 1];
        let paragraph_start = sentence.line == 1
            || raw_lines[sentence.line - 2].trim().is_empty()
            || heading_lines.binary_search(&(sentence.line - 1)).is_ok();
        if !paragraph_start
            || sentence.line_byte_start != raw_line.len() - raw_line.trim_start().len()
        {
            continue;
        }
        let Some((start, end)) = explanation_preview_span(&sentence.tokens) else {
            continue;
        };
        let section = heading_lines.partition_point(|line| *line < sentence.line);
        sections.entry(section).or_default().push(AggregateHit {
            line: sentence.line,
            excerpt: sentence.excerpt(start, end),
            span: sentence.span(&raw_lines, start, end),
            related_lines: vec![sentence.line],
        });
    }
    sections
        .into_values()
        .flat_map(|hits| {
            aggregate_pattern_finding(hits, 3, "repeated_explanation_preview", |count, related| {
                format!(
                    "同じ節の段落頭で、説明・紹介・解説を予告する形態素列が{count}回ある（閾値3回以上）。必要な案内は残し、予告の代わりに内容から書き始められる箇所を確認してください。対応箇所: {related}"
                )
            })
        })
        .collect()
}

fn explanation_preview_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let first = tokens.first()?;
    if !matches!(first.surface.as_str(), "本節" | "本章" | "ここ" | "以下")
        || tokens.get(1)?.surface != "で"
        || tokens.get(2)?.surface != "は"
    {
        return None;
    }
    let last = tokens.len().checked_sub(1)?;
    let verb_index = if tokens[last].dictionary_form() == "ます" {
        last.checked_sub(1)?
    } else {
        last
    };
    let verb = &tokens[verb_index];
    let noun_index = verb_index.checked_sub(1)?;
    let noun = &tokens[noun_index];
    if noun_index <= 3
        || verb.pos(0) != "動詞"
        || verb.dictionary_form() != "する"
        || !tokens[last].pos(5).starts_with("終止形")
        || noun.pos(2) != "サ変可能"
        || !matches!(noun.dictionary_form(), "説明" | "紹介" | "解説")
    {
        return None;
    }
    // 別の主語や引用、否定・過去・可能を含む述語を予告へまとめない。
    // 述語の後ろは「ます」だけを許すので、活用に伴う意味の違いを保てる。
    if tokens[3..noun_index]
        .iter()
        .enumerate()
        .any(|(index, token)| {
            (token.pos(0) == "助詞" && matches!(token.surface.as_str(), "は" | "が"))
                || (matches!(token.pos(0), "記号" | "補助記号")
                    && !(index == 0 && token.surface == "、"))
        })
    {
        return None;
    }
    Some((first.byte_start, tokens[last].byte_end))
}

pub(super) fn negative_listing_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let hits = tokenized
        .windows(3)
        .filter_map(|window| {
            let [first, second, assertion] = window else {
                return None;
            };
            if !copular_negation(&first.tokens)
                || !copular_negation(&second.tokens)
                || !short_affirmative(&assertion.tokens)
                || blank_line_between(first.line, second.line, raw_lines)
                || blank_line_between(second.line, assertion.line, raw_lines)
            {
                return None;
            }

            let start = significant_tokens(&first.tokens).first()?.byte_start;
            let end = significant_tokens(&assertion.tokens).last()?.byte_end;
            let span = make_span(
                raw_lines,
                first.line,
                first.line_byte_start + start,
                assertion.line,
                assertion.line_byte_start + end,
            );
            Some(AggregateHit {
                line: first.line,
                excerpt: [first, second, assertion]
                    .map(|sentence| abbreviated(&sentence.raw_text, 28))
                    .join("。"),
                span,
                related_lines: vec![first.line, second.line, assertion.line],
            })
        })
        .collect::<Vec<_>>();
    aggregate_pattern_finding(hits, 1, "negative_listing", |count, related| {
        format!(
            "否定を2文続けてから短い肯定文へ焦点を移す並びが{}箇所ある。対比や選択肢の絞り込みとして意図した修辞かを確認する実験的検出。対応箇所: {related}",
            count
        )
    })
}

fn abbreviated(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut abbreviated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        abbreviated.push('…');
    }
    abbreviated
}

fn copular_negation(tokens: &[Morpheme]) -> bool {
    let tokens = significant_tokens(tokens)
        .iter()
        .filter(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .collect::<Vec<_>>();
    let Some(negative) = tokens.iter().rposition(|token| {
        token.dictionary_form() == "ない"
            || (token.dictionary_form() == "ぬ" && token.surface == "ん")
    }) else {
        return false;
    };
    if tokens.len().saturating_sub(negative) > 3 {
        return false;
    }
    tokens[..negative].windows(2).rev().take(5).any(|pair| {
        matches!(pair[0].surface.as_str(), "で" | "じゃ")
            && matches!(pair[1].surface.as_str(), "は" | "も")
    })
}

fn short_affirmative(tokens: &[Morpheme]) -> bool {
    let content = significant_tokens(tokens)
        .iter()
        .filter(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .collect::<Vec<_>>();
    !content.is_empty()
        && content.len() <= 8
        && !content.iter().any(|token| {
            token.dictionary_form() == "ない"
                || (token.dictionary_form() == "ぬ" && token.surface == "ん")
        })
}

fn blank_line_between(first_line: usize, second_line: usize, raw_lines: &[&str]) -> bool {
    if second_line <= first_line + 1 {
        return false;
    }
    raw_lines[first_line..second_line - 1]
        .iter()
        .any(|line| line.trim().is_empty())
}

pub(super) fn translationese_morph_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (sentence, index, token) in token_positions(tokenized) {
        if let Some((byte_start, byte_end, pattern)) =
            abstract_motsu_candidate(&sentence.tokens, index)
        {
            let mut finding = Finding::new(
                sentence.line,
                "translationese_morph",
                sentence.excerpt(byte_start, byte_end),
                "info",
                format!(
                    "品詞列マッチ: {pattern}。抽象的な内容と「持つ」の組み合わせを読み直す候補"
                ),
            );
            finding.span = sentence.span(raw_lines, byte_start, byte_end);
            findings.push(finding);
        }

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
    findings
}

fn abstract_motsu_candidate(
    tokens: &[Morpheme],
    index: usize,
) -> Option<(usize, usize, &'static str)> {
    let first = tokens.get(index)?;
    let second = tokens.get(index + 1)?;
    let third = tokens.get(index + 2);

    if second.pos(0) == "助詞" && second.surface == "を" {
        let third = third?;
        if third.pos(0) == "動詞" && third.dictionary_form() == "持つ" {
            if first.pos(0) == "名詞" && first.dictionary_form() == "意味" {
                return Some((first.byte_start, third.byte_end, "意味+を+持つ"));
            }
            if first.pos(0) == "助詞" && first.surface == "か" {
                return Some((first.byte_start, third.byte_end, "疑問節末のか+を+持つ"));
            }
        }
    }

    if first.surface == "持てる" && second.pos(0) == "名詞" && second.dictionary_form() == "未決"
    {
        return Some((first.byte_start, second.byte_end, "持てる+未決"));
    }

    if first.surface == "持て" && second.surface == "る" {
        let third = third?;
        if third.pos(0) == "名詞" && third.dictionary_form() == "未決" {
            return Some((first.byte_start, third.byte_end, "持てる+未決"));
        }
    }

    None
}

pub(super) fn technical_ambiguity_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        for (index, token) in sentence.tokens.iter().enumerate() {
            if matches!(token.surface.as_str(), "この" | "その" | "あの")
                && sentence
                    .tokens
                    .get(index + 1)
                    .is_some_and(|next| next.pos(0) == "名詞" && next.dictionary_form() == "こと")
            {
                let predicate_count = sentence.tokens[..index]
                    .iter()
                    .filter(|candidate| candidate.pos(0) == "動詞")
                    .count();
                if predicate_count >= 2 {
                    let end = sentence.tokens[index + 1].byte_end;
                    let mut finding = Finding::new(
                        sentence.line,
                        "demonstrative_reference",
                        sentence.excerpt(token.byte_start, end),
                        "info",
                        format!(
                            "品詞列マッチ: 同じ文の前方に動詞が{predicate_count}個あり、その後に「{}こと」がある。指示先を読み直す候補",
                            token.surface
                        ),
                    );
                    finding.span = sentence.span(raw_lines, token.byte_start, end);
                    findings.push(finding);
                }
            }

            if token.surface != "それぞれ" {
                continue;
            }
            let preceding_separator = sentence.tokens[..index]
                .iter()
                .rposition(is_enumeration_separator);
            if preceding_separator.is_none()
                || sentence.tokens[index + 1..]
                    .iter()
                    .any(is_enumeration_separator)
            {
                continue;
            }
            let separator = preceding_separator.expect("separator exists");
            let start = sentence.tokens[separator.saturating_sub(1)].byte_start;
            let end = sentence.tokens[index + 1..]
                .iter()
                .find(|candidate| candidate.pos(0) == "動詞")
                .map_or(token.byte_end, |candidate| candidate.byte_end);
            let mut finding = Finding::new(
                sentence.line,
                "respectively_scope",
                sentence.excerpt(start, end),
                "info",
                "品詞列マッチ: 列挙の後に「それぞれ」があり、後方に対応する列挙がない。どの要素を一つずつ扱うか読み直す候補",
            );
            finding.span = sentence.span(raw_lines, start, end);
            findings.push(finding);
        }
    }
    findings
}

fn is_enumeration_separator(token: &Morpheme) -> bool {
    (token.pos(0) == "助詞" && matches!(token.surface.as_str(), "と" | "や"))
        || matches!(token.surface.as_str(), "、" | "，" | ",")
}

/// テスト結果を色で、ソフトウェアの公開を物流語で表す技術現場の言い回し。
/// 単語だけでは本来の表示色や物理配送も拾うため、同じ節の対象語と状態語を
/// 組み合わせ、表示・配送を示す名詞がある文は除外する。
pub(super) fn technical_jargon_metaphor_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        if !contains_any(&sentence.tokens, DISPLAY_NOUNS)
            && let Some((start, end)) = color_status_span(&sentence.tokens)
        {
            let mut finding = Finding::new(
                sentence.line,
                "technical_jargon_metaphor",
                sentence.excerpt(start, end),
                "info",
                "テストや検査の状態を色で表す技術現場の言い回し。何が通ったか、成功したかを直接書けるか確認してください",
            );
            finding.span = sentence.span(raw_lines, start, end);
            findings.push(finding);
        }

        if !contains_any(&sentence.tokens, PHYSICAL_SHIPMENT_NOUNS)
            && let Some((start, end)) = software_shipment_span(&sentence.tokens)
        {
            let mut finding = Finding::new(
                sentence.line,
                "technical_jargon_metaphor",
                sentence.excerpt(start, end),
                "info",
                "ソフトウェアの公開を物流語で表す技術現場の言い回し。公開、配布、リリースなど具体的な動作を書けるか確認してください",
            );
            finding.span = sentence.span(raw_lines, start, end);
            findings.push(finding);
        }
    }
    findings
}

/// 抽象名詞を動作主にして移動や効果を表す、技術文書の翻訳調候補。
/// 物理的な運搬や一般的な「効く」を拾わないよう、観測済みの格関係と
/// 数量名詞の直後に限定する。
pub(super) fn abstract_predicate_metaphor_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        if let Some((start, end)) = abstract_transport_span(&sentence.tokens) {
            let mut finding = Finding::new(
                sentence.line,
                "abstract_metaphor",
                sentence.excerpt(start, end),
                "info",
                "抽象名詞と移動動詞の組み合わせ。意図や判断がどのように実装へ反映されるか、具体的に書けるか確認してください",
            );
            finding.span = sentence.span(raw_lines, start, end);
            findings.push(finding);
        }

        if let Some((start, end)) = abstract_effect_span(&sentence.tokens) {
            let mut finding = Finding::new(
                sentence.line,
                "abstract_metaphor",
                sentence.excerpt(start, end),
                "info",
                "抽象的な尺度と「で効く」の組み合わせ。何がどのように影響するか、具体的に書けるか確認してください",
            );
            finding.span = sentence.span(raw_lines, start, end);
            findings.push(finding);
        }
    }
    findings
}

fn contains_any(tokens: &[Morpheme], candidates: &[&str]) -> bool {
    position_any(tokens, candidates).is_some()
}

fn position_any(tokens: &[Morpheme], candidates: &[&str]) -> Option<usize> {
    tokens.iter().position(|token| {
        candidates.contains(&token.dictionary_form())
            || candidates.contains(&token.surface.as_str())
    })
}

fn color_status_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let subject = position_any(tokens, TECHNICAL_STATUS_NOUNS)?;
    let color = tokens.iter().position(|token| {
        matches!(token.dictionary_form(), "緑" | "グリーン")
            && matches!(token.pos(0), "名詞" | "形状詞")
    })?;
    if subject >= color || punctuation_between(tokens, subject, color) {
        return None;
    }
    let following = &tokens[color + 1..];
    let state = following.iter().take(4).position(|token| {
        matches!(
            token.dictionary_form(),
            "だ" | "です" | "なる" | "戻す" | "保つ" | "まま"
        ) || token.surface == "で"
    })? + color
        + 1;
    if punctuation_between(tokens, color, state) {
        return None;
    }
    Some((tokens[subject].byte_start, tokens[state].byte_end))
}

fn software_shipment_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let shipment = tokens
        .iter()
        .position(|token| token.dictionary_form() == "出荷")?;
    let software = position_any(&tokens[..shipment], SOFTWARE_NOUNS)
        .filter(|index| !punctuation_between(tokens, *index, shipment));
    let scoped_state = tokens[..shipment]
        .iter()
        .enumerate()
        .position(|(index, token)| {
            token.dictionary_form() == "状態"
                && index > 0
                && tokens[index - 1].surface == "どの"
                && !punctuation_between(tokens, index, shipment)
        });
    let action = tokens[shipment + 1..]
        .iter()
        .position(|token| token.pos(0) == "動詞")
        .map(|index| index + shipment + 1);
    if !action.is_some_and(|index| matches!(tokens[index].dictionary_form(), "する" | "止める"))
    {
        return None;
    }
    let stop = action.filter(|index| tokens[*index].dictionary_form() == "止める");
    let decide = tokens
        .iter()
        .position(|token| token.dictionary_form() == "決める");
    let context =
        software.or_else(|| scoped_state.filter(|_| stop.is_some() && decide.is_some()))?;
    let end = action.expect("shipment action exists");
    Some((
        tokens[context.min(shipment)].byte_start,
        tokens[end].byte_end,
    ))
}

fn abstract_transport_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let verb = tokens
        .iter()
        .position(|token| token.dictionary_form() == "運ぶ")?;
    let destination = tokens[..verb]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            ABSTRACT_TRANSPORT_OBJECTS
                .contains(&token.dictionary_form())
                .then_some(index)
                .filter(|index| {
                    tokens
                        .get(*index + 1)
                        .is_some_and(|particle| matches!(particle.surface.as_str(), "へ" | "に"))
                })
        })?;
    let object = tokens[..destination]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            ABSTRACT_TRANSPORT_OBJECTS
                .contains(&token.dictionary_form())
                .then_some(index)
                .filter(|index| {
                    tokens
                        .get(*index + 1)
                        .is_some_and(|particle| particle.surface == "を")
                })
        })?;
    let subject = tokens[..object]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            ABSTRACT_TRANSPORT_SUBJECTS
                .contains(&token.dictionary_form())
                .then_some(index)
                .filter(|index| {
                    tokens
                        .get(*index + 1)
                        .is_some_and(|particle| matches!(particle.surface.as_str(), "は" | "が"))
                })
        })?;
    if punctuation_between(tokens, subject, object)
        || punctuation_between(tokens, object, destination)
        || punctuation_between(tokens, destination, verb)
    {
        return None;
    }
    Some((tokens[subject].byte_start, tokens[verb].byte_end))
}

fn abstract_effect_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let verb = tokens
        .iter()
        .position(|token| token.dictionary_form() == "効く")?;
    let particle = verb.checked_sub(1)?;
    let quantity = particle.checked_sub(1)?;
    if tokens[particle].surface != "で"
        || tokens[quantity].pos(0) != "名詞"
        || !QUANTITY_NOUN_ENDINGS.iter().any(|ending| {
            tokens[quantity].dictionary_form() == *ending
                || tokens[quantity].surface.ends_with(ending)
        })
    {
        return None;
    }
    let (subject, subject_particle) = abstract_effect_subject(tokens, quantity)?;
    if subject_particle >= quantity
        || (subject_particle + 1..quantity).any(|index| {
            let token = &tokens[index];
            token.pos(0) == "助詞"
                && (token.surface == "が"
                    || (token.surface == "は" && tokens[index - 1].pos(0) != "助詞"))
        })
    {
        return None;
    }
    Some((tokens[subject].byte_start, tokens[verb].byte_end))
}

fn abstract_effect_subject(tokens: &[Morpheme], before: usize) -> Option<(usize, usize)> {
    tokens[..before]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            if !ABSTRACT_EFFECT_SUBJECTS.contains(&token.dictionary_form()) {
                return None;
            }
            let suffix = tokens.get(index + 1);
            let particle_index = if token.dictionary_form() == "複雑"
                && suffix.is_some_and(|candidate| {
                    candidate.surface == "さ" && candidate.pos(0) == "接尾辞"
                }) {
                index + 2
            } else {
                index + 1
            };
            tokens.get(particle_index).and_then(|particle| {
                matches!(particle.surface.as_str(), "は" | "が").then_some((index, particle_index))
            })
        })
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

/// サ変名詞+を+行う型の冗長（「検証を行う」→「検証する」）。
/// 名詞とを、をと行うが隣接する場合だけ対象にし、受身（行われる）と
/// 名詞がサ変可能でない場合（「祭りを行う」）は対象外。textlintの
/// ai-tech-writing-guidelineが指摘する冗長クラスのうち、形態素で
/// 決定的に判定できる部分だけを扱う。
pub(super) fn redundant_light_verb_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (sentence, index, noun) in token_positions(tokenized) {
        let Some(particle) = sentence.tokens.get(index + 1) else {
            continue;
        };
        let Some(verb) = sentence.tokens.get(index + 2) else {
            continue;
        };
        let verbal_noun =
            noun.pos(0) == "名詞" && (noun.pos(2) == "サ変可能" || noun.pos(2) == "サ変形状詞可能");
        if !verbal_noun
            || particle.pos(0) != "助詞"
            || particle.surface != "を"
            || verb.pos(0) != "動詞"
            || !matches!(verb.dictionary_form(), "行う" | "行なう")
        {
            continue;
        }
        // 受身・使役（行われる、行わせる）は言い換えの意味が変わるため対象外
        let passive_or_causative = sentence.tokens.get(index + 3).is_some_and(|next| {
            matches!(
                next.dictionary_form(),
                "れる" | "られる" | "せる" | "させる"
            )
        });
        if passive_or_causative {
            continue;
        }
        let mut finding = Finding::new(
            sentence.line,
            "redundant_light_verb",
            sentence.excerpt(noun.byte_start, verb.byte_end),
            "info",
            format!(
                "サ変名詞+を+行う型の冗長候補: 「{}を{}」は「{}する」へ畳める。名詞の動作性を活かす方が簡潔（意図的な文体なら維持する）",
                noun.surface, verb.surface, noun.surface
            ),
        );
        finding.span = sentence.span(raw_lines, noun.byte_start, verb.byte_end);
        finding.suggestion = light_verb_suggestion(sentence, raw_lines, particle, verb);
        findings.push(finding);
    }
    findings
}

/// 抽象的な対象を物体になぞらえ、判断内容を曖昧にする可能性がある名詞句。
/// 候補語だけでは本来の意味での用例まで拾うため、述語として使われる場合か、
/// 抽象名詞から「の」で修飾される場合に限定する。
pub(super) fn abstract_metaphor_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (sentence, index, token) in token_positions(tokenized) {
        if token.pos(0) != "名詞" || !ABSTRACT_METAPHOR_NOUNS.contains(&token.dictionary_form()) {
            continue;
        }

        let abstract_genitive = index >= 2
            && sentence.tokens[index - 1].surface == "の"
            && sentence.tokens[index - 2].pos(0) == "名詞"
            && is_abstract_context_noun(&sentence.tokens[index - 2]);
        let predicate_end = metaphor_predicate_end(&sentence.tokens, index);
        if !abstract_genitive
            && (predicate_end.is_none() || !has_abstract_context_before(&sentence.tokens, index))
        {
            continue;
        }

        let byte_start = if index >= 2 && sentence.tokens[index - 1].surface == "の" {
            sentence.tokens[index - 2].byte_start
        } else {
            token.byte_start
        };
        let byte_end = predicate_end
            .map(|end| sentence.tokens[end].byte_end)
            .unwrap_or(token.byte_end);
        let mut finding = Finding::new(
            sentence.line,
            "abstract_metaphor",
            sentence.excerpt(byte_start, byte_end),
            "info",
            format!(
                "抽象比喩の可能性: 「{}」。判断対象・判断基準・具体的な効果を明記してください",
                token.surface
            ),
        );
        finding.span = sentence.span(raw_lines, byte_start, byte_end);
        findings.push(finding);
    }
    findings
}

fn is_abstract_context_noun(token: &Morpheme) -> bool {
    ABSTRACT_CONTEXT_NOUNS.iter().any(|candidate| {
        token.dictionary_form() == *candidate || token.surface.ends_with(candidate)
    })
}

fn has_abstract_context_before(tokens: &[Morpheme], noun_index: usize) -> bool {
    tokens[..noun_index]
        .iter()
        .rev()
        .take_while(|token| !matches!(token.pos(0), "記号" | "補助記号"))
        .take(12)
        .any(|token| token.pos(0) == "名詞" && is_abstract_context_noun(token))
}

fn metaphor_predicate_end(tokens: &[Morpheme], noun_index: usize) -> Option<usize> {
    let first = tokens.get(noun_index + 1)?;
    if matches!(first.dictionary_form(), "だ" | "です") {
        return Some(noun_index + 1);
    }

    let second = tokens.get(noun_index + 2)?;
    if matches!(first.surface.as_str(), "に" | "と") && second.dictionary_form() == "なる" {
        return Some(noun_index + 2);
    }
    if first.surface == "で" && second.dictionary_form() == "ある" {
        return Some(noun_index + 2);
    }
    if first.surface == "と" && second.dictionary_form() == "する" {
        let third = tokens.get(noun_index + 3)?;
        if third.surface == "て" {
            return Some(noun_index + 3);
        }
    }
    None
}

/// 「を+行う」の活用形ごとに機械的に安全な置換だけを提案する。
/// 行う→する、行い→し、行っ→し。それ以外の活用（行わ、行え等）は
/// 提案しない。preimageがraw行と一致しない場合も提案しない。
fn light_verb_suggestion(
    sentence: &TokenizedSentence,
    raw_lines: &[&str],
    particle: &Morpheme,
    verb: &Morpheme,
) -> Option<Suggestion> {
    let replacement = match verb.surface.as_str() {
        "行う" | "行なう" => "する",
        "行い" | "行ない" => "し",
        "行っ" | "行なっ" => "し",
        _ => return None,
    };
    let expected = format!("{}{}", particle.surface, verb.surface);
    let line_start = sentence.line_byte_start + particle.byte_start;
    let line_end = sentence.line_byte_start + verb.byte_end;
    let matches_raw = raw_lines
        .get(sentence.line - 1)
        .and_then(|raw_line| raw_line.get(line_start..line_end))
        .is_some_and(|slice| slice == expected);
    if !matches_raw {
        return None;
    }
    Some(Suggestion {
        span: sentence.span(raw_lines, particle.byte_start, verb.byte_end)?,
        preimage: expected,
        replacement: replacement.to_owned(),
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
