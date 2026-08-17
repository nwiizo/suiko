use std::collections::{BTreeMap, HashMap};

use regex::Regex;
use serde::Serialize;

use crate::Error;
use crate::morphology::Morphology;
use crate::text::{heading, is_heading, is_list_item, mask_html_comments};

const TEMPLATE_HEADING_WORDS: &[&str] = &[
    "はじめに",
    "背景",
    "概要",
    "本記事について",
    "この記事について",
    "まとめと今後",
    "今後の展望",
    "今後の課題",
    "今後について",
    "まとめ",
    "おわりに",
    "終わりに",
    "さいごに",
    "最後に",
    "結論",
    "総括",
    "conclusion",
    "introduction",
    "summary",
];

#[derive(Clone, Debug, Serialize)]
pub struct OutlineEntry {
    pub line: usize,
    pub kind: String,
    pub level: Option<usize>,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TemplateHit {
    pub line: usize,
    pub text: String,
    pub matched: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct HeadingGroupStats {
    pub count: usize,
    pub length_mean: f64,
    pub length_cv: f64,
    pub nominal_ending_ratio: f64,
    pub dominant_pos_signature_ratio: f64,
    pub template_hits: Vec<TemplateHit>,
    pub structural_pattern_ratio: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HeadingStats {
    pub total_headings: usize,
    pub level_distribution: BTreeMap<String, usize>,
    pub by_level: BTreeMap<String, HeadingGroupStats>,
    pub overall: HeadingGroupStats,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutlineReport {
    pub outline: Vec<OutlineEntry>,
    pub heading_stats: HeadingStats,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockKind {
    Lead,
    Bullets,
    Quote,
    Table,
}

fn block_kind(line: &str) -> BlockKind {
    let trimmed = line.trim_start();
    if is_list_item(line) {
        BlockKind::Bullets
    } else if trimmed.starts_with('>') {
        BlockKind::Quote
    } else if (trimmed.starts_with('|') && trimmed.matches('|').count() >= 2)
        || (trimmed.contains('|')
            && trimmed
                .chars()
                .all(|ch| ch.is_whitespace() || matches!(ch, '|' | '-' | ':')))
    {
        BlockKind::Table
    } else {
        BlockKind::Lead
    }
}

fn flush_block(buffer: &mut Vec<(usize, String)>, output: &mut Vec<OutlineEntry>) {
    let Some((first_line_no, first_line)) = buffer.first() else {
        return;
    };
    match block_kind(first_line) {
        BlockKind::Bullets => {
            let count = buffer.iter().filter(|(_, line)| is_list_item(line)).count();
            output.push(OutlineEntry {
                line: *first_line_no,
                kind: "bullets".to_owned(),
                level: None,
                text: format!("(箇条書き {count} 項目)"),
            });
        }
        BlockKind::Lead => {
            let end = first_line
                .char_indices()
                .find(|(_, ch)| matches!(ch, '。' | '！' | '？'))
                .map_or(first_line.len(), |(byte, ch)| byte + ch.len_utf8());
            let text = first_line[..end].trim();
            if !text.is_empty() {
                output.push(OutlineEntry {
                    line: *first_line_no,
                    kind: "lead".to_owned(),
                    level: None,
                    text: text.to_owned(),
                });
            }
        }
        BlockKind::Quote | BlockKind::Table => {}
    }
    buffer.clear();
}

pub fn build_outline(raw_text: &str) -> Vec<OutlineEntry> {
    let text = mask_html_comments(raw_text);
    let mut output = Vec::new();
    let mut buffer = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut front_matter = false;

    for (index, line) in text.split('\n').enumerate() {
        let line_no = index + 1;
        if index == 0 && line.trim() == "---" {
            front_matter = true;
            continue;
        }
        if front_matter {
            if line.trim() == "---" {
                front_matter = false;
            }
            continue;
        }

        let trimmed = line.trim_start();
        let fence_run = trimmed
            .chars()
            .next()
            .filter(|ch| *ch == '`' || *ch == '~')
            .map(|ch| {
                (
                    ch,
                    trimmed
                        .chars()
                        .take_while(|candidate| candidate == &ch)
                        .count(),
                )
            });
        if let Some((open_char, open_len)) = fence {
            if fence_run.is_some_and(|(ch, len)| {
                ch == open_char && len >= open_len && trimmed[len..].trim().is_empty()
            }) {
                fence = None;
            }
            continue;
        }
        if let Some((ch, len)) = fence_run.filter(|(_, len)| *len >= 3) {
            flush_block(&mut buffer, &mut output);
            fence = Some((ch, len));
            continue;
        }
        if line.trim().is_empty() {
            flush_block(&mut buffer, &mut output);
            continue;
        }
        if is_heading(line) {
            flush_block(&mut buffer, &mut output);
            if let Some((level, text)) = heading(line) {
                output.push(OutlineEntry {
                    line: line_no,
                    kind: "heading".to_owned(),
                    level: Some(level),
                    text,
                });
            }
            continue;
        }

        if let Some((_, first)) = buffer.first() {
            let current = block_kind(first);
            let next = block_kind(line);
            let indented_bullet_continuation = current == BlockKind::Bullets
                && next == BlockKind::Lead
                && line.starts_with(char::is_whitespace);
            if current != next && !indented_bullet_continuation {
                flush_block(&mut buffer, &mut output);
            }
        }
        buffer.push((line_no, line.to_owned()));
    }
    flush_block(&mut buffer, &mut output);
    output
}

fn round(value: f64, digits: i32) -> f64 {
    let scale = 10_f64.powi(digits);
    (value * scale).round() / scale
}

fn stripped_heading(text: &str) -> String {
    text.trim_start_matches(|ch: char| {
        ch.is_whitespace()
            || ch.is_ascii_digit()
            || ch.is_ascii_punctuation()
            || matches!(
                ch,
                '０'..='９' | '．' | '、' | '（' | '）' | '【' | '】' | '・'
            )
    })
    .trim()
    .to_lowercase()
}

fn template_word(text: &str) -> Option<&'static str> {
    let stripped = stripped_heading(text);
    TEMPLATE_HEADING_WORDS
        .iter()
        .copied()
        .find(|word| stripped.starts_with(&word.to_lowercase()))
}

fn structural_pattern(text: &str) -> bool {
    let numbered =
        Regex::new(r"^\s*([0-9０-９]+[.).、]|[①-⑳])\s*\S").expect("valid numbered-heading regex");
    let bracketed =
        Regex::new(r"^\s*[【\[［(（].+[】\]］)）]\s*$").expect("valid bracketed-heading regex");
    let towa = Regex::new(r".+とは[?？]?\s*$").expect("valid definition-heading regex");
    numbered.is_match(text) || bracketed.is_match(text) || towa.is_match(text)
}

fn summarize(
    headings: &[&OutlineEntry],
    morphology: &Morphology,
) -> Result<HeadingGroupStats, Error> {
    if headings.is_empty() {
        return Ok(HeadingGroupStats {
            count: 0,
            length_mean: 0.0,
            length_cv: 0.0,
            nominal_ending_ratio: 0.0,
            dominant_pos_signature_ratio: 0.0,
            template_hits: Vec::new(),
            structural_pattern_ratio: 0.0,
        });
    }
    let count = headings.len();
    let lengths = headings
        .iter()
        .map(|heading| heading.text.chars().count() as f64)
        .collect::<Vec<_>>();
    let mean = lengths.iter().sum::<f64>() / count as f64;
    let cv = if mean > 0.0 && count > 1 {
        let variance = lengths
            .iter()
            .map(|length| (length - mean).powi(2))
            .sum::<f64>()
            / count as f64;
        variance.sqrt() / mean
    } else {
        0.0
    };
    let mut nominal_count = 0;
    let mut signatures = HashMap::<Vec<String>, usize>::new();
    for heading in headings {
        let tokens = morphology.tokenize(&heading.text)?;
        if tokens
            .iter()
            .rev()
            .find(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
            .is_some_and(|token| token.pos(0) == "名詞")
        {
            nominal_count += 1;
        }
        let signature = tokens
            .iter()
            .filter(|token| {
                matches!(
                    token.pos(0),
                    "名詞" | "動詞" | "形容詞" | "副詞" | "接頭詞" | "接頭辞"
                )
            })
            .map(|token| token.pos(0).to_owned())
            .collect::<Vec<_>>();
        if !signature.is_empty() {
            *signatures.entry(signature).or_default() += 1;
        }
    }
    let dominant = signatures.values().copied().max().unwrap_or_default();
    let template_hits = headings
        .iter()
        .filter_map(|heading| {
            template_word(&heading.text).map(|matched| TemplateHit {
                line: heading.line,
                text: heading.text.clone(),
                matched: matched.to_owned(),
            })
        })
        .collect();
    let structural_count = headings
        .iter()
        .filter(|heading| structural_pattern(&heading.text))
        .count();
    Ok(HeadingGroupStats {
        count,
        length_mean: round(mean, 2),
        length_cv: round(cv, 3),
        nominal_ending_ratio: round(nominal_count as f64 / count as f64, 3),
        dominant_pos_signature_ratio: round(dominant as f64 / count as f64, 3),
        template_hits,
        structural_pattern_ratio: round(structural_count as f64 / count as f64, 3),
    })
}

pub fn analyze(raw_text: &str, morphology: &Morphology) -> Result<OutlineReport, Error> {
    let outline = build_outline(raw_text);
    let headings = outline
        .iter()
        .filter(|entry| entry.kind == "heading")
        .collect::<Vec<_>>();
    let mut grouped = BTreeMap::<usize, Vec<&OutlineEntry>>::new();
    for entry in &headings {
        grouped
            .entry(entry.level.unwrap_or_default())
            .or_default()
            .push(entry);
    }
    let level_distribution = grouped
        .iter()
        .map(|(level, entries)| (level.to_string(), entries.len()))
        .collect();
    let mut by_level = BTreeMap::new();
    for (level, entries) in grouped {
        by_level.insert(level.to_string(), summarize(&entries, morphology)?);
    }
    let overall = summarize(&headings, morphology)?;
    Ok(OutlineReport {
        heading_stats: HeadingStats {
            total_headings: headings.len(),
            level_distribution,
            by_level,
            overall,
        },
        outline,
    })
}
