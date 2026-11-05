//! Markdownの区切りを、診断と書式修正で共用する。
use std::ops::Range;
use std::sync::LazyLock;

use pulldown_cmark::{Event, Parser, Tag};
use regex::Regex;

use crate::text::{
    mask_emphasis_delimiters, mask_inline_spans, prepare_lint_text, protected_lines,
};

pub(crate) fn is_punctuation(ch: Option<char>) -> bool {
    static PUNCTUATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\p{P}\p{S}]$").expect("valid Unicode categories"));
    ch.is_some_and(|ch| PUNCTUATION.is_match(ch.encode_utf8(&mut [0; 4])))
}

fn whitespace(ch: Option<char>) -> bool {
    ch.is_none_or(char::is_whitespace)
}

pub(crate) fn left_flanking(before: Option<char>, after: Option<char>) -> bool {
    !whitespace(after) && (!is_punctuation(after) || whitespace(before) || is_punctuation(before))
}

pub(crate) fn bold_renders(
    previous: Option<char>,
    first: Option<char>,
    last: Option<char>,
    next: Option<char>,
    underscore: bool,
) -> bool {
    let open = left_flanking(previous, first);
    let close = left_flanking(next, last);
    if underscore {
        open && (!left_flanking(first, previous) || is_punctuation(previous))
            && close
            && (!left_flanking(last, next) || is_punctuation(next))
    } else {
        open && close
    }
}

/// 対応が読み取れる同一行の候補。エスケープ、コード、リンク先は除く。
pub(crate) fn bold_ranges(line: &str) -> Vec<Range<usize>> {
    static BOLD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\*{2,}[^*\n]+\*{2,}|_{2,}[^_\n]+_{2,}").expect("valid bold regex")
    });
    let masked = mask_inline_spans(line);
    let code = Parser::new(line)
        .into_offset_iter()
        .filter_map(|(event, range)| matches!(event, Event::Code(_)).then_some(range))
        .collect::<Vec<_>>();
    BOLD.find_iter(line)
        .filter(|found| {
            if code
                .iter()
                .any(|range| range.contains(&found.start()) || range.contains(&(found.end() - 1)))
            {
                return false;
            }
            let escaped =
                |at: usize| line[..at].bytes().rev().take_while(|b| *b == b'\\').count() % 2 != 0;
            !escaped(found.start())
                && !escaped(found.end() - 2)
                && masked.as_bytes()[found.start()] == line.as_bytes()[found.start()]
                && masked.as_bytes()[found.end() - 1] == line.as_bytes()[found.end() - 1]
        })
        .map(|found| found.range())
        .collect()
}

pub(crate) fn quotation_ranges(text: &str) -> Vec<Range<usize>> {
    static QUOTES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"「[^」]*」|『[^』]*』|“[^”]*”|"[^"\n]*""#).expect("valid quotation regex")
    });
    QUOTES.find_iter(text).map(|found| found.range()).collect()
}

pub(crate) fn duplicate_commas(text: &str) -> Vec<Range<usize>> {
    static COMMAS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("、{2,}").expect("valid commas regex"));
    let quotes = quotation_ranges(text);
    let masked = mask_inline_spans(text);
    COMMAS
        .find_iter(&masked)
        .filter(|found| !quotes.iter().any(|quote| quote.contains(&found.start())))
        .map(|found| found.range())
        .collect()
}

/// 文を分割する前に区切りをマスクする。句点をまたぐ強調でも孤立した記号を文にしない。
/// 表示されない2連の太字も、明確な対応がある場合だけ推敲上の区切りとして扱う。
pub(crate) fn mask_review_emphasis(raw: &str) -> String {
    let mut masked = mask_emphasis_delimiters(raw);
    let mut offset = 0;
    for line in raw.split('\n') {
        for range in bold_ranges(line) {
            let span = &line[range.clone()];
            if span.starts_with("**") && !span.starts_with("***") && !span.ends_with("***") {
                masked.replace_range(offset + range.start..offset + range.start + 2, "  ");
                masked.replace_range(offset + range.end - 2..offset + range.end, "  ");
            }
        }
        offset += line.len() + 1;
    }
    masked
}

/// 書き換えるのは開閉が明確な2連アスタリスクだけ。複雑な入れ子は診断に残す。
pub(crate) fn fix_bold(raw: &str) -> String {
    let (prepared, _) = prepare_lint_text(raw);
    let lines = raw.split('\n').collect::<Vec<_>>();
    let protected = protected_lines(&lines);
    let mut exclusions = quotation_ranges(raw);
    for (event, range) in Parser::new(raw).into_offset_iter() {
        if matches!(
            event,
            Event::Start(
                Tag::CodeBlock(_)
                    | Tag::BlockQuote(_)
                    | Tag::HtmlBlock
                    | Tag::Link { .. }
                    | Tag::Image { .. }
            ) | Event::Code(_)
                | Event::Html(_)
                | Event::InlineHtml(_)
        ) {
            exclusions.push(range);
        }
    }
    let mut output = raw.to_owned();
    let mut edits = Vec::new();
    let mut offset = 0;
    for (index, line) in lines.iter().enumerate() {
        let end = offset + line.len();
        if !protected[index] && !line.starts_with("    ") && !line.starts_with('\t') {
            let candidates = bold_ranges(&prepared[offset..end]);
            let rendered = mask_emphasis_delimiters(line);
            for range in candidates {
                let text = &line[range.clone()];
                if !text.starts_with("**")
                    || text.starts_with("***")
                    || text.ends_with("***")
                    || rendered[range.start..range.start + 2] == *"  "
                    || rendered[range.end - 2..range.end] == *"  "
                    || exclusions.iter().any(|span| {
                        span.contains(&(offset + range.start))
                            || span.contains(&(offset + range.end - 1))
                    })
                {
                    continue;
                }
                let inner = &text[2..text.len() - 2];
                // 空白や改行を伴う曖昧な区切りは自動修正せず、lintへ残す。
                if inner.is_empty() || inner.trim() != inner {
                    continue;
                }
                let body = inner
                    .trim_end_matches(['。', '、', '！', '？', '!', '?', '：', ':', '；', ';']);
                if body.is_empty() {
                    continue;
                }
                let trailing = &inner[body.len()..];
                let before = line[..range.start].chars().next_back();
                let after = line[range.end..].chars().next();
                let left = if left_flanking(before, body.chars().next()) {
                    ""
                } else {
                    " "
                };
                let next = trailing.chars().next().or(after);
                let right = if left_flanking(next, body.chars().next_back()) {
                    ""
                } else {
                    " "
                };
                let replacement = format!("{left}**{body}**{right}{trailing}");
                let mut trial = (*line).to_owned();
                trial.replace_range(range.clone(), &replacement);
                let masked = mask_emphasis_delimiters(&trial);
                let opening = range.start + left.len();
                let closing = opening + 2 + body.len();
                if masked[opening..opening + 2] == *"  " && masked[closing..closing + 2] == *"  " {
                    edits.push((offset + range.start..offset + range.end, replacement));
                }
            }
        }
        offset = end + 1;
    }
    for (range, replacement) in edits.into_iter().rev() {
        output.replace_range(range, &replacement);
    }
    output
}
