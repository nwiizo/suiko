use std::sync::LazyLock;

use regex::Regex;

/// 表示される文字の数。Markdownの区切り・リンク先・空白は数えない。
/// 読点候補と読解負荷で同じ基準を使い、語や記号の追加は引き続き数える。
pub(crate) fn reading_length(text: &str) -> usize {
    pulldown_cmark::Parser::new(text)
        .map(|event| match event {
            pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text) => {
                text.chars().filter(|ch| !ch.is_whitespace()).count()
            }
            _ => 0,
        })
        .sum()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sentence {
    pub line: usize,
    pub text: String,
    pub raw_text: String,
    /// 文を閉じた句点類。行末まで句点がない文はNone。
    pub end_mark: Option<char>,
    /// 行内での文本文の開始byte offset。マスクはbyte長を保存するため、
    /// masked行で計算した値はraw行の同じ位置を指す。
    pub line_byte_start: usize,
}

pub fn numbered_lines(text: &str) -> Vec<(usize, &str)> {
    text.split('\n')
        .enumerate()
        .map(|(i, line)| (i + 1, line))
        .collect()
}

fn heading_prefix(line: &str) -> Option<(&str, usize)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    let valid_prefix = (1..=6).contains(&level)
        && trimmed
            .as_bytes()
            .get(level)
            .is_none_or(u8::is_ascii_whitespace);
    valid_prefix.then_some((trimmed, level))
}

pub fn is_heading(line: &str) -> bool {
    heading_prefix(line).is_some()
}

pub fn heading(line: &str) -> Option<(usize, String)> {
    let (trimmed, level) = heading_prefix(line)?;
    Some((
        level,
        trimmed[level..]
            .trim()
            .trim_end_matches('#')
            .trim()
            .to_owned(),
    ))
}

pub fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    if ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return true;
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0
        && matches!(trimmed.as_bytes().get(digits), Some(b'.' | b')'))
        && trimmed
            .as_bytes()
            .get(digits + 1)
            .is_some_and(u8::is_ascii_whitespace)
}

pub fn mask_html_comments(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    let mut in_comment = false;
    while !rest.is_empty() {
        if in_comment {
            if let Some(end) = rest.find("-->") {
                for ch in rest[..end + 3].chars() {
                    if ch == '\n' {
                        output.push('\n');
                    } else {
                        output.push_str(&" ".repeat(ch.len_utf8()));
                    }
                }
                rest = &rest[end + 3..];
                in_comment = false;
            } else {
                for ch in rest.chars() {
                    if ch == '\n' {
                        output.push('\n');
                    } else {
                        output.push_str(&" ".repeat(ch.len_utf8()));
                    }
                }
                break;
            }
        } else if let Some(start) = rest.find("<!--") {
            output.push_str(&rest[..start]);
            rest = &rest[start..];
            in_comment = true;
        } else {
            output.push_str(rest);
            break;
        }
    }
    output
}

/// mask_markdownが行単位で抑制した本文外の行数。
#[derive(Clone, Copy, Debug, Default)]
pub struct MaskStats {
    pub reference_lines: usize,
    pub code_annotation_lines: usize,
}

#[derive(Default)]
pub(crate) struct ReviewMaskStats {
    pub ignored_regions: usize,
    pub ignored_lines: usize,
    pub hatena_spans: usize,
    pub invalid_directives: Vec<usize>,
}

static HATENA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(?:asin:[^]\n]+:(?:detail|title)|f:id:[^]\n]+:plain|:contents|https?://[^]\n]+:embed(?::cite)?)\]").expect("valid Hatena notation")
});

fn mask_hatena(line: &str) -> String {
    HATENA
        .replace_all(line, |m: &regex::Captures<'_>| " ".repeat(m[0].len()))
        .into_owned()
}

/// lint専用の除外。diffの原文保護には適用しない。無効な指示は本文を隠さない。
pub(crate) fn prepare_lint_text(raw: &str) -> (String, ReviewMaskStats) {
    let lines = raw.split('\n').collect::<Vec<_>>();
    let protected = protected_lines(&lines);
    let mut stats = ReviewMaskStats::default();
    let mut excluded = vec![false; lines.len()];
    let mut stack = Vec::new();
    let mut invalid_region = false;
    let mut in_comment = false;
    for (index, line) in lines.iter().enumerate() {
        if protected[index] {
            continue;
        }
        let trimmed = line.trim();
        if !in_comment && trimmed == "<!-- suiko:ignore-start -->" {
            if !stack.is_empty() {
                invalid_region = true;
            }
            stack.push(index);
            continue;
        }
        if !in_comment && trimmed == "<!-- suiko:ignore-end -->" {
            if let Some(start) = stack.pop() {
                if invalid_region {
                    stats.invalid_directives.extend([start + 1, index + 1]);
                } else {
                    excluded[start + 1..index].fill(true);
                    stats.ignored_regions += 1;
                    stats.ignored_lines += index - start - 1;
                }
                if stack.is_empty() {
                    invalid_region = false;
                }
            } else {
                stats.invalid_directives.push(index + 1);
            }
            continue;
        }
        let mut rest = *line;
        loop {
            let delimiter = if in_comment { "-->" } else { "<!--" };
            let Some(at) = rest.find(delimiter) else {
                break;
            };
            rest = &rest[at + delimiter.len()..];
            in_comment = !in_comment;
        }
    }
    stats
        .invalid_directives
        .extend(stack.into_iter().map(|index| index + 1));
    stats.invalid_directives.sort_unstable();
    stats.invalid_directives.dedup();
    let result = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if excluded[index] {
                return " ".repeat(line.len());
            }
            if protected[index] {
                return (*line).to_owned();
            }
            stats.hatena_spans += HATENA.find_iter(line).count();
            mask_hatena(line)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (result, stats)
}

/// 英文の引用だけを読解負荷から除く。訳や日本語の技術用語は残す。
pub(crate) fn mask_english_quotes(raw: &str) -> String {
    static QUOTES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"「[^「」\n]+」|『[^『』\n]+』|“[^“”\n]+”|"[^"\n]+""#)
            .expect("valid quotation regex")
    });
    QUOTES
        .replace_all(raw, |capture: &regex::Captures<'_>| {
            let quote = &capture[0];
            let letters = quote.chars().filter(char::is_ascii_alphabetic).count();
            let japanese = quote
                .chars()
                .any(|ch| matches!(ch, 'ぁ'..='ん' | 'ァ'..='ヶ' | '一'..='鿿'));
            if !japanese && letters >= 20 && quote.split_whitespace().count() >= 4 {
                " ".repeat(quote.len())
            } else {
                quote.to_owned()
            }
        })
        .into_owned()
}

/// CommonMarkで実際に強調になった区切りだけを空白にし、元のbyte位置を保つ。
pub(crate) fn mask_emphasis_delimiters(raw: &str) -> String {
    use pulldown_cmark::{Event, Parser, Tag};
    let mut masked = raw.to_owned();
    for (event, range) in Parser::new(raw).into_offset_iter() {
        let width = match event {
            Event::Start(Tag::Strong) => 2,
            Event::Start(Tag::Emphasis) => 1,
            _ => continue,
        };
        let Some(source) = raw.get(range.clone()) else {
            continue;
        };
        if source.len() >= width * 2
            && (source.starts_with(&"*".repeat(width)) || source.starts_with(&"_".repeat(width)))
        {
            let marker = &source[..width];
            if source.ends_with(marker) {
                masked.replace_range(range.start..range.start + width, &" ".repeat(width));
                masked.replace_range(range.end - width..range.end, &" ".repeat(width));
            }
        }
    }
    masked
}

pub(crate) fn without_emphasis(raw: &str) -> String {
    let masked = mask_emphasis_delimiters(raw);
    raw.char_indices()
        .filter_map(|(index, ch)| {
            if matches!(ch, '*' | '_') && masked.as_bytes()[index] == b' ' {
                None
            } else {
                Some(ch)
            }
        })
        .collect()
}

/// `[1] 著者名…`のような参考文献リスト行、`[^1]: …`の脚注定義行。
/// 行頭にある場合だけ本文から外す。文中の`研究[1]は…`のような参照は残す。
pub fn is_reference_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return false;
    };
    let Some(close) = rest.find(']') else {
        return false;
    };
    let label = &rest[..close];
    let after = &rest[close + 1..];
    let numeric = !label.is_empty() && label.bytes().all(|byte| byte.is_ascii_digit());
    let footnote = label.starts_with('^') && label.len() > 1;
    (numeric && after.starts_with([' ', '\t', ':'])) || (footnote && after.starts_with(':'))
}

/// `#A 説明`のようなManning式コード注釈行。見出しは`#`の直後が空白なので
/// 衝突しない。
pub fn is_code_annotation_line(line: &str) -> bool {
    let bytes = line.trim_start().as_bytes();
    bytes.len() >= 3 && bytes[0] == b'#' && bytes[1].is_ascii_uppercase() && bytes[2] == b' '
}

fn is_reference_heading(title: &str) -> bool {
    matches!(
        title.trim().to_ascii_lowercase().as_str(),
        "参考文献" | "引用文献" | "references" | "bibliography"
    )
}

pub fn mask_markdown_structure(text: &str) -> String {
    mask_markdown(text, false).0
}

pub fn mask_markdown_structure_with_stats(text: &str) -> (String, MaskStats) {
    mask_markdown(text, false)
}

pub fn mask_markdown_structure_preserving_headings(text: &str) -> String {
    mask_markdown(text, true).0
}

/// 推敲・資料照合用。本文に加え、見出し・箇条書き・表の可視文字を残す。
/// 引用、コード、front matter、コメント、参考文献は従来どおり除外する。
pub(crate) fn revision_text(raw: &str) -> String {
    let visible = mask_html_comments(raw);
    let raw_lines = visible.split('\n').collect::<Vec<_>>();
    let protected = protected_lines(&raw_lines);
    let masked = mask_markdown_structure(&visible);
    let mut reference_level = None;
    masked
        .split('\n')
        .enumerate()
        .map(|(index, masked)| {
            let line = raw_lines[index];
            if protected[index] {
                return String::new();
            }
            if let Some((level, title)) = heading(line) {
                if is_reference_heading(&title) {
                    reference_level = Some(level);
                } else if reference_level.is_some_and(|old| level <= old) {
                    reference_level = None;
                }
            }
            if reference_level.is_some() {
                return String::new();
            }
            let trimmed = line.trim_start();
            let table = trimmed.starts_with('|') && trimmed.matches('|').count() >= 2;
            if table
                && trimmed
                    .chars()
                    .all(|ch| ch.is_whitespace() || matches!(ch, '|' | '-' | ':'))
            {
                return String::new();
            }
            if is_heading(line) || is_list_item(line) || table {
                mask_inline_spans(&strip_line_prefix(line))
            } else {
                masked.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 表のセルを別の文として扱う。ASCIIの区切りに置き換え、原文のbyte位置を保つ。
pub(crate) fn revision_sentences(raw: &str) -> Vec<Sentence> {
    let visible = revision_text(raw);
    let cells = visible
        .split('\n')
        .map(|line| {
            if line.trim_start().starts_with('|') {
                line.replace('|', "!")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let mut sentences = sentences_with_raw(&cells, raw);
    for sentence in &mut sentences {
        // 除いたMarkdown記号の分だけrawの開始位置も進め、抜粋とspanを揃える。
        if let Some(source) = raw_lines.get(sentence.line - 1).and_then(|line| {
            line.get(sentence.line_byte_start..sentence.line_byte_start + sentence.text.len())
        }) {
            sentence.raw_text = source.to_owned();
        }
    }
    sentences
}

static INLINE_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"``[^\n]*?``|`[^`\n]+`").expect("valid inline-code regex"));
static INLINE_HTML: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"</?[A-Za-z][A-Za-z0-9-]*(?:\s[^>\n]*)?/?>").expect("valid HTML-tag regex")
});
static LINK_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\]\()([^)]*)(\))").expect("valid Markdown-link regex"));

/// 1行の中のインラインコード、HTMLタグ、リンクURLを同じbyte長の空白へ置き換える。
pub(crate) fn mask_inline_spans(line: &str) -> String {
    let masked = mask_hatena(line);
    let line = masked.as_str();
    let no_code = INLINE_CODE.replace_all(line, |captures: &regex::Captures<'_>| {
        " ".repeat(captures[0].len())
    });
    let no_html = INLINE_HTML.replace_all(&no_code, |captures: &regex::Captures<'_>| {
        " ".repeat(captures[0].len())
    });
    LINK_URL
        .replace_all(&no_html, |captures: &regex::Captures<'_>| {
            format!(
                "{}{}{}",
                &captures[1],
                " ".repeat(captures[2].len()),
                &captures[3]
            )
        })
        .into_owned()
}

/// 行頭のMarkdown記号（箇条書きの印、見出しの#、引用の>）を同じbyte長の空白へ置き換え、
/// 本文だけを残す。該当しない行はそのまま返す。
pub(crate) fn strip_line_prefix(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let body = if is_list_item(line) {
        let marker = if trimmed.starts_with(['-', '*', '+']) {
            1
        } else {
            trimmed.bytes().take_while(u8::is_ascii_digit).count() + 1
        };
        let rest = &trimmed[marker..];
        let rest_trimmed = rest.trim_start();
        let checkbox = ["[ ] ", "[x] ", "[X] "]
            .iter()
            .find(|prefix| rest_trimmed.starts_with(*prefix))
            .map_or(0, |prefix| prefix.len());
        marker + (rest.len() - rest_trimmed.len()) + checkbox
    } else if let Some((level, _)) = heading(line) {
        level + trimmed[level..].len() - trimmed[level..].trim_start().len()
    } else if trimmed.starts_with('>') {
        let mut offset = 0;
        let bytes = trimmed.as_bytes();
        while offset < bytes.len() && matches!(bytes[offset], b'>' | b' ' | b'\t') {
            offset += 1;
        }
        offset
    } else {
        return line.to_owned();
    };
    format!("{}{}", " ".repeat(indent + body), &trimmed[body..])
}

fn mask_markdown(text: &str, preserve_headings: bool) -> (String, MaskStats) {
    let text = mask_html_comments(text);
    let embed_citation =
        Regex::new(r"^\[https?://[^]\n]+:embed:cite\]$").expect("valid embed-citation regex");
    let mut masked = Vec::new();
    let mut stats = MaskStats::default();
    let mut fence: Option<(char, usize)> = None;
    let mut front_matter = false;
    let mut reference_section_level = None;

    for (index, line) in text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        if index == 0 && line.trim() == "---" {
            front_matter = true;
            masked.push(String::new());
            continue;
        }
        if front_matter {
            masked.push(String::new());
            if line.trim() == "---" {
                front_matter = false;
            }
            continue;
        }

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
            masked.push(String::new());
            if fence_run.is_some_and(|(ch, len)| ch == open_char && len >= open_len) {
                fence = None;
            }
            continue;
        }
        if let Some((ch, len)) = fence_run.filter(|(_, len)| *len >= 3) {
            fence = Some((ch, len));
            masked.push(String::new());
            continue;
        }

        let current_heading = heading(line);
        if let Some((level, title)) = &current_heading {
            if is_reference_heading(title) {
                reference_section_level = Some(*level);
            } else if reference_section_level.is_some_and(|reference| *level <= reference) {
                reference_section_level = None;
            }
        }
        if reference_section_level.is_some() && current_heading.is_none() {
            if !trimmed.is_empty() {
                stats.reference_lines += 1;
            }
            masked.push(String::new());
            continue;
        }

        let is_table = (trimmed.starts_with('|') && trimmed.matches('|').count() >= 2)
            || (trimmed.contains('|')
                && trimmed
                    .chars()
                    .all(|ch| ch.is_whitespace() || matches!(ch, '|' | '-' | ':')));
        if (!preserve_headings && is_heading(line))
            || is_list_item(line)
            || trimmed.starts_with('>')
            || is_table
            || embed_citation.is_match(trimmed)
        {
            masked.push(String::new());
            continue;
        }
        if is_reference_line(line) {
            stats.reference_lines += 1;
            masked.push(String::new());
            continue;
        }
        if is_code_annotation_line(line) {
            stats.code_annotation_lines += 1;
            masked.push(String::new());
            continue;
        }

        masked.push(mask_inline_spans(line));
    }
    (masked.join("\n"), stats)
}

pub fn sentences(text: &str) -> Vec<Sentence> {
    sentences_with_raw(text, text)
}

pub fn sentences_with_raw(text: &str, raw_text: &str) -> Vec<Sentence> {
    let mut output = Vec::new();
    let raw_lines = raw_text.split('\n').collect::<Vec<_>>();
    for (line_no, line) in numbered_lines(text) {
        let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or(line);
        let mut start = 0;
        for (byte, ch) in line.char_indices() {
            if matches!(ch, '。' | '！' | '？' | '!' | '?') {
                let end = byte;
                let segment = &line[start..end];
                let sentence = segment.trim();
                if !sentence.is_empty() {
                    let leading = segment.len() - segment.trim_start().len();
                    output.push(Sentence {
                        line: line_no,
                        text: sentence.to_owned(),
                        raw_text: raw_line
                            .get(start..end)
                            .unwrap_or(sentence)
                            .trim()
                            .to_owned(),
                        end_mark: Some(ch),
                        line_byte_start: start + leading,
                    });
                }
                start = byte + ch.len_utf8();
            }
        }
        let segment = &line[start..];
        let tail = segment.trim();
        if !tail.is_empty() {
            let leading = segment.len() - segment.trim_start().len();
            output.push(Sentence {
                line: line_no,
                text: tail.to_owned(),
                raw_text: raw_line.get(start..).unwrap_or(tail).trim().to_owned(),
                end_mark: None,
                line_byte_start: start + leading,
            });
        }
    }
    output
}

pub fn excerpt_around(
    line: &str,
    byte_start: usize,
    byte_len: usize,
    context_chars: usize,
) -> String {
    let char_start = line[..byte_start].chars().count();
    let match_chars = line[byte_start..byte_start + byte_len].chars().count();
    let chars = line.chars().collect::<Vec<_>>();
    let from = char_start.saturating_sub(context_chars);
    let to = (char_start + match_chars + context_chars).min(chars.len());
    chars[from..to].iter().collect()
}

/// 各行がフェンス付きコードブロックの中（開始・終了の行を含む）かを返す。
pub(crate) fn fenced_lines(lines: &[&str]) -> Vec<bool> {
    let mut fence: Option<(char, usize)> = None;
    lines
        .iter()
        .map(|line| {
            let trimmed = line.trim_start();
            let fence_run = trimmed
                .chars()
                .next()
                .filter(|ch| *ch == '`' || *ch == '~')
                .map(|ch| (ch, trimmed.chars().take_while(|c| c == &ch).count()));
            if let Some((open_char, open_len)) = fence {
                if fence_run.is_some_and(|(ch, len)| ch == open_char && len >= open_len) {
                    fence = None;
                }
                true
            } else if let Some((ch, len)) = fence_run.filter(|(_, len)| *len >= 3) {
                fence = Some((ch, len));
                true
            } else {
                false
            }
        })
        .collect()
}

/// 散文の検査が読み飛ばす行。フェンス付きコードブロックと、先頭のfront matter（閉じの`---`まで。
/// 閉じがなければ文書の終わりまで）。
pub(crate) fn protected_lines(lines: &[&str]) -> Vec<bool> {
    let mut protected = fenced_lines(lines);
    if lines.first().is_some_and(|line| line.trim() == "---") {
        let end = lines
            .iter()
            .skip(1)
            .position(|line| line.trim() == "---")
            .map_or(lines.len(), |index| index + 2);
        protected.iter_mut().take(end).for_each(|flag| *flag = true);
    }
    protected
}

/// 行の中のインラインコードのbyte範囲。
pub(crate) fn inline_code_ranges(line: &str) -> Vec<std::ops::Range<usize>> {
    INLINE_CODE
        .find_iter(line)
        .map(|found| found.range())
        .collect()
}
