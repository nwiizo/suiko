//! 型の族: 同じ言い回しの型が近くで繰り返される箇所を、最初の箇所へ集約して示す
//! （自己ラベル、説明の予告、否定の列挙、技術文書の同じ型の説明）。

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use crate::morphology::Morpheme;

use super::morph::{TokenizedSentence, significant_tokens};
use super::{Finding, Span, make_span};

const SELF_LABEL_EVALUATIONS: &[&str] = &["必要", "重要", "大切", "面白い", "興味深い"];

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

/// 同じ節の5文以内に3回現れる型を集約し、離れた説明の累積を避ける。
pub(super) fn technical_repetition_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
    raw: &str,
) -> Vec<Finding> {
    let dash = Regex::new(r"[—―]+").expect("valid em-dash regex");
    let structural = crate::text::mask_markdown_structure_preserving_headings(raw);
    let heading_lines = structural
        .lines()
        .enumerate()
        .filter_map(|(index, line)| crate::text::is_heading(line).then_some(index + 1))
        .collect::<Vec<_>>();
    let mut distinctions = Vec::new();
    let mut dashes = Vec::new();
    for (index, sentence) in tokenized
        .iter()
        .filter(|sentence| sentence.text.chars().any(char::is_alphanumeric))
        .enumerate()
    {
        if !matches!(sentence.end_mark, Some('?' | '？'))
            && let Some(token) = sentence.tokens.iter().find(|token| {
                token.pos(0) == "名詞"
                    && token.dictionary_form() == "別物"
                    && matches!(
                        sentence.text[token.byte_end..].trim_matches(['*', '_']),
                        "だ" | "です" | "である"
                    )
            })
        {
            distinctions.push((
                index,
                AggregateHit {
                    line: sentence.line,
                    excerpt: sentence.excerpt(token.byte_start, sentence.text.len()),
                    span: sentence.span(raw_lines, token.byte_start, sentence.text.len()),
                    related_lines: vec![sentence.line],
                },
            ));
        }
        if let Some(found) = dash.find_iter(&sentence.text).find(|found| {
            let before = sentence.text[..found.start()]
                .trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, '*' | '_'))
                .chars()
                .next_back();
            let after = sentence.text[found.end()..]
                .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '*' | '_'))
                .chars()
                .next();
            before.zip(after).is_some_and(|(left, right)| {
                left.is_alphanumeric()
                    && right.is_alphanumeric()
                    && !(left.is_numeric() && right.is_numeric())
            })
        }) {
            dashes.push((
                index,
                AggregateHit {
                    line: sentence.line,
                    excerpt: sentence.excerpt(found.start(), found.end()),
                    span: sentence.span(raw_lines, found.start(), found.end()),
                    related_lines: vec![sentence.line],
                },
            ));
        }
    }
    let mut findings = aggregate_pattern_finding(
        clustered_repetition_hits(distinctions, &heading_lines),
        3,
        "repeated_distinction",
        |count, related| {
            format!(
                "同じ節の5文以内に「別物だ／です／である」で締める文が3文以上ある。該当する反復は計{count}文。比較の説明や文末の続き方を確認してください。必要な比較は残せます。対応箇所: {related}"
            )
        },
    );
    findings.extend(aggregate_pattern_finding(clustered_repetition_hits(dashes, &heading_lines), 3, "repeated_em_dash", |count, related| {
        format!("同じ節の5文以内に文中のダッシュ（—／―）を使う文が3文以上ある。該当する反復は計{count}文。挿入や言い換えが続く箇所を読み直し、句点や括弧で区切ると読みやすいか確認してください。対応箇所: {related}")
    }));
    findings
}

// nwiizo-coding-style: 近接を同じ節の5文以内で近似する;
// 読み直し範囲が広すぎる実例が集まったら、反例とともに範囲を見直す。
fn clustered_repetition_hits(
    hits: Vec<(usize, AggregateHit)>,
    heading_lines: &[usize],
) -> Vec<AggregateHit> {
    let mut retained = BTreeSet::new();
    for window in hits.windows(3) {
        let first = &window[0];
        let last = &window[2];
        if last.0 - first.0 < 5
            && heading_lines.partition_point(|line| *line < first.1.line)
                == heading_lines.partition_point(|line| *line < last.1.line)
        {
            retained.extend(window.iter().map(|(index, _)| *index));
        }
    }
    hits.into_iter()
        .filter_map(|(index, hit)| retained.contains(&index).then_some(hit))
        .collect()
}
