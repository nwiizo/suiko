//! 宣言した項目数と直後の箇条書きの項目数の照合。
//! 「次の3点」のような限定した宣言だけを拾い、直後のリストの同じ階層の
//! 項目を数える。確実に数えられない形は判定しない（eval/six-axis-morphology-design.md）。

use std::sync::LazyLock;

use regex::Regex;

use crate::text::{excerpt_around, is_heading, is_list_item};

use super::patterns::fenced_lines;
use super::{Finding, make_span};

const CATEGORY: &str = "declared_item_count_mismatch";

/// 宣言の直後に続くと、数が上限・概数・例示になる語。
const APPROXIMATE_SUFFIXES: &[&str] = &[
    "以上",
    "以下",
    "未満",
    "程度",
    "前後",
    "ほど",
    "余り",
    "あまり",
    "など",
    "等",
    "ずつ",
];

static DECLARATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:次の|以下の|下記の)([0-9０-９]+|[一二三四五六七八九十]+)(つ|点|項目|個|件|ステップ|段階|種類|手順|条件)",
    )
    .expect("valid declaration regex")
});

struct Declaration {
    line: usize,
    byte_start: usize,
    byte_end: usize,
    count: usize,
    label: String,
}

fn parse_number(text: &str) -> Option<usize> {
    if text
        .chars()
        .all(|ch| ch.is_ascii_digit() || ('０'..='９').contains(&ch))
    {
        let ascii = text
            .chars()
            .map(|ch| {
                if ch.is_ascii_digit() {
                    ch
                } else {
                    char::from_u32(ch as u32 - '０' as u32 + '0' as u32).unwrap_or('0')
                }
            })
            .collect::<String>();
        return ascii.parse().ok();
    }
    let digit = |ch: char| "一二三四五六七八九".find(ch).map(|index| index / 3 + 1);
    let chars = text.chars().collect::<Vec<_>>();
    match chars.iter().position(|ch| *ch == '十') {
        None if chars.len() == 1 => digit(chars[0]),
        None => None,
        Some(pos) => {
            let tens = match pos {
                0 => 1,
                1 => digit(chars[0])?,
                _ => return None,
            };
            let ones = match chars.len() - pos - 1 {
                0 => 0,
                1 => digit(chars[pos + 1])?,
                _ => return None,
            };
            Some(tens * 10 + ones)
        }
    }
}

fn indent_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

fn is_ordered_marker(line: &str) -> bool {
    line.trim_start()
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn is_block_boundary(line: &str) -> bool {
    let trimmed = line.trim_start();
    is_heading(line) || trimmed.starts_with('>') || trimmed.starts_with('|')
}

/// 段落の最後の文から、限定した宣言を一つ探す。文末が句点・コロン以外なら判定しない。
fn declaration_in(line_no: usize, line: &str) -> Option<Declaration> {
    let body = line.trim_end();
    if !body.ends_with(['。', '：', ':']) {
        return None;
    }
    let without_end = body.trim_end_matches(['。', '：', ':']);
    let sentence_start = without_end.rfind(['。', '！', '？']).map_or(0, |pos| {
        pos + without_end[pos..].chars().next().map_or(0, char::len_utf8)
    });
    let sentence = &body[sentence_start..];
    let found = DECLARATION.captures_iter(sentence).last()?;
    let whole = found.get(0)?;
    let rest = &sentence[whole.end()..];
    if APPROXIMATE_SUFFIXES
        .iter()
        .any(|suffix| rest.starts_with(suffix))
        || sentence[..whole.start()].contains("うち")
    {
        return None;
    }
    let count = parse_number(found.get(1)?.as_str())?;
    Some(Declaration {
        line: line_no,
        byte_start: sentence_start + whole.start(),
        byte_end: sentence_start + whole.end(),
        count,
        label: format!("{}{}", found.get(1)?.as_str(), found.get(2)?.as_str()),
    })
}

/// `start`から始まるリストの、同じ階層の項目の行番号(0始まり)。
/// 数えられない形なら`None`を返す。
fn top_level_items(lines: &[&str], fenced: &[bool], start: usize) -> Option<Vec<usize>> {
    let base_indent = indent_width(lines[start]);
    let ordered = is_ordered_marker(lines[start]);
    let mut items = vec![start];
    let mut index = start + 1;
    while index < lines.len() {
        let line = lines[index];
        if fenced[index] {
            if indent_width(line) <= base_indent {
                break;
            }
            index += 1;
            continue;
        }
        if is_blank(line) {
            let next = (index + 1..lines.len()).find(|&next| !is_blank(lines[next]));
            match next {
                Some(next) if indent_width(lines[next]) >= base_indent && !fenced[next] => {
                    if indent_width(lines[next]) == base_indent && !is_list_item(lines[next]) {
                        break;
                    }
                    index = next;
                    continue;
                }
                Some(next) if fenced[next] && indent_width(lines[next]) > base_indent => {
                    index = next;
                    continue;
                }
                _ => break,
            }
        }
        let indent = indent_width(line);
        if indent < base_indent || (indent == base_indent && is_block_boundary(line)) {
            break;
        }
        if indent == base_indent && is_list_item(line) {
            if is_ordered_marker(line) != ordered {
                return None;
            }
            items.push(index);
        }
        index += 1;
    }
    let ellipsis = items.iter().any(|&item| {
        let text = lines[item].trim_start();
        let text = text
            .split_once(char::is_whitespace)
            .map_or("", |(_, rest)| rest)
            .trim();
        matches!(text, "…" | "……" | "..." | "⋯") || text.is_empty()
    });
    (!ellipsis).then_some(items)
}

/// リスト直前の段落の最後の行。空行1つまでを挟める。
fn lead_line(lines: &[&str], fenced: &[bool], list_start: usize) -> Option<usize> {
    let candidate = match list_start.checked_sub(1)? {
        prev if is_blank(lines[prev]) => prev.checked_sub(1)?,
        prev => prev,
    };
    let line = lines[candidate];
    let usable = !fenced[candidate]
        && !is_blank(line)
        && !is_list_item(line)
        && !is_block_boundary(line)
        && indent_width(line) == 0;
    usable.then_some(candidate)
}

pub(super) fn declared_item_count_findings(masked: &str, raw_lines: &[&str]) -> Vec<Finding> {
    let lines = masked.split('\n').collect::<Vec<_>>();
    let fenced = fenced_lines(&lines);
    let mut findings = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let starts_list = !fenced[index]
            && is_list_item(lines[index])
            && indent_width(lines[index]) == 0
            && index
                .checked_sub(1)
                .is_none_or(|prev| is_blank(lines[prev]) || !is_list_item(lines[prev]));
        if !starts_list {
            index += 1;
            continue;
        }
        let items = top_level_items(&lines, &fenced, index);
        let next = items
            .as_ref()
            .and_then(|items| items.last())
            .map_or(index + 1, |last| last + 1);
        if let (Some(items), Some(lead)) = (items, lead_line(&lines, &fenced, index))
            && let Some(declaration) = declaration_in(lead + 1, lines[lead])
            && declaration.count != items.len()
            && (1..=30).contains(&declaration.count)
        {
            let raw_line = raw_lines.get(lead).copied().unwrap_or_default();
            let mut finding = Finding::new(
                declaration.line,
                CATEGORY,
                excerpt_around(
                    raw_line,
                    declaration.byte_start,
                    declaration.byte_end - declaration.byte_start,
                    10,
                )
                .trim(),
                "info",
                format!(
                    "{}と予告していますが、直後の箇条書きは{}項目です。項目の不足か、予告の数字を確認してください",
                    declaration.label,
                    items.len()
                ),
            );
            finding.span = make_span(
                raw_lines,
                declaration.line,
                declaration.byte_start,
                declaration.line,
                declaration.byte_end,
            );
            finding.related_lines = Some(items.iter().map(|item| item + 1).collect());
            findings.push(finding);
        }
        index = next.max(index + 1);
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::parse_number;

    #[test]
    fn parses_arabic_fullwidth_and_kanji_numbers() {
        assert_eq!(parse_number("3"), Some(3));
        assert_eq!(parse_number("１２"), Some(12));
        assert_eq!(parse_number("三"), Some(3));
        assert_eq!(parse_number("十"), Some(10));
        assert_eq!(parse_number("十二"), Some(12));
        assert_eq!(parse_number("二十"), Some(20));
        assert_eq!(parse_number("三十五"), Some(35));
        assert_eq!(parse_number("三三"), None);
    }
}
