//! 原稿（Markdown）の読み取り: 本文・引用・注・参考文献・同期用の本文を取り出す。

use std::collections::BTreeMap;

use regex::Regex;

use crate::text::{heading, mask_html_comments};

pub(super) fn normalized(text: &str) -> String {
    text.chars()
        .map(|ch| if ch == '：' { ':' } else { ch })
        .filter(|ch| !ch.is_whitespace() && !ch.is_control())
        .filter(|ch| !matches!(ch, '*' | '_' | '`'))
        .collect()
}

#[derive(Debug)]
pub(super) struct SourceView {
    pub(super) main_markdown: String,
    pub(super) main_text: String,
    pub(super) citation_text: String,
    pub(super) sync_text: String,
    pub(super) prose_paragraphs: Vec<String>,
    pub(super) references: Vec<String>,
    pub(super) notes: BTreeMap<String, String>,
    pub(super) note_references: BTreeMap<String, usize>,
}

fn normalized_section_title(title: &str) -> String {
    title
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '-' | '―' | '—' | '–' | '━' | 'ー' | '_' | '=' | '~' | '＊' | '*' | '・'
                )
        })
        .to_ascii_lowercase()
}

fn is_reference_heading(title: &str) -> bool {
    matches!(
        normalized_section_title(title).as_str(),
        "参考文献" | "引用文献" | "references" | "bibliography"
    )
}

fn is_note_heading(title: &str) -> bool {
    matches!(
        normalized_section_title(title).as_str(),
        "注" | "注記" | "notes" | "endnotes"
    )
}

fn ascii_digits(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '０'..='９' => char::from_u32(ch as u32 - '０' as u32 + '0' as u32).unwrap_or(ch),
            _ => ch,
        })
        .collect()
}

pub(super) fn normalize_note_marker(marker: &str) -> String {
    let marker = marker.trim().trim_start_matches("[^").trim_end_matches(']');
    let marker = marker.strip_prefix('注').unwrap_or(marker);
    let marker = marker.trim_end_matches(['）', ')', '．', '.']).trim();
    ascii_digits(marker)
}

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed
        .chars()
        .next()
        .filter(|ch| matches!(ch, '`' | '~'))?;
    let length = trimmed.chars().take_while(|ch| *ch == marker).count();
    (length >= 3).then_some((marker, length))
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|')
        && trimmed
            .chars()
            .all(|ch| ch.is_whitespace() || matches!(ch, '|' | '-' | ':'))
}

fn markdown_content_lines(text: &str) -> impl Iterator<Item = String> + '_ {
    let image = Regex::new(r"!\[([^]]*)\]\([^)]+\)").expect("valid image regex");
    let link = Regex::new(r"\[([^]]+)\]\([^)]+\)").expect("valid link regex");
    let footnote = Regex::new(r"\[\^[^]]+\]").expect("valid footnote regex");
    text.lines()
        .filter(|line| !is_table_separator(line))
        .map(|line| heading(line).map_or_else(|| line.to_owned(), |(_, title)| title))
        .map(move |line| image.replace_all(&line, "$1").into_owned())
        .map(move |line| link.replace_all(&line, "$1").into_owned())
        .map(move |line| footnote.replace_all(&line, "").into_owned())
}

pub(super) fn plain_markdown(text: &str) -> String {
    let html = Regex::new(r"</?[A-Za-z][^>]*>").expect("valid HTML regex");
    markdown_content_lines(text)
        .map(|line| html.replace_all(&line, "").into_owned())
        .map(|line| line.replace(['*', '`', '|'], " "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn citation_markdown(text: &str) -> String {
    markdown_content_lines(text)
        .map(|line| line.replace(['*', '`'], " "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn prose_blocks(markdown: &str) -> Vec<String> {
    markdown
        .split("\n\n")
        .map(str::trim)
        .filter(|block| {
            !block.is_empty()
                && !block.starts_with('#')
                && !block.lines().all(|line| line.trim_start().starts_with('|'))
                && !block
                    .lines()
                    .all(|line| line.trim_start().starts_with("[^"))
        })
        .map(plain_markdown)
        .filter(|block| !block.trim().is_empty())
        .collect()
}

fn reference_entries(lines: &[String]) -> Vec<String> {
    let entry = Regex::new(r"^\s*(?:[-*+]\s+|[0-9０-９]+[.)）．]\s*|[\[［][0-9０-９]+[\]］]\s*)")
        .expect("valid bibliography entry regex");
    let mut entries = Vec::new();
    let mut current = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            if !current.is_empty() {
                entries.push(plain_markdown(&current.join("\n")));
                current.clear();
            }
            continue;
        }
        let starts_entry =
            entry.is_match(line) || (!current.is_empty() && !line.starts_with(char::is_whitespace));
        if starts_entry && !current.is_empty() {
            entries.push(plain_markdown(&current.join("\n")));
            current.clear();
        }
        let line = entry.replace(line, "").into_owned();
        current.push(line);
    }
    if !current.is_empty() {
        entries.push(plain_markdown(&current.join("\n")));
    }
    entries
}

pub(super) fn source_view(source: &str, omit_prefixes: &[String]) -> SourceView {
    let source = mask_html_comments(source);
    let lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    let note_definition = Regex::new(r"^\[\^([^]]+)\]:\s*(.*)$").expect("valid note regex");
    let note_reference = Regex::new(r"\[\^([^]]+)\]").expect("valid note reference regex");
    let japanese_note_definition =
        Regex::new(r"^\s*注\s*([0-9０-９]+)[）)．.]\s*(.*)$").expect("valid Japanese note regex");
    let japanese_note_reference =
        Regex::new(r"注\s*([0-9０-９]+)[）)]").expect("valid Japanese note reference regex");
    let mut main_lines = Vec::new();
    let mut citation_lines = Vec::new();
    let mut sync_lines = Vec::new();
    let mut reference_lines = Vec::new();
    let mut notes = BTreeMap::new();
    let mut note_references = BTreeMap::new();
    let mut front_matter = false;
    let mut fence: Option<(char, usize)> = None;
    let mut reference_level = None;
    let mut note_level = None;
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        let trimmed = line.trim();
        if index == 0 && trimmed == "---" {
            front_matter = true;
            index += 1;
            continue;
        }
        if front_matter {
            if trimmed == "---" {
                front_matter = false;
            }
            index += 1;
            continue;
        }
        if let Some((open, length)) = fence {
            if fence_marker(line)
                .is_some_and(|(close, close_length)| close == open && close_length >= length)
            {
                fence = None;
            }
            index += 1;
            continue;
        }
        if let Some(marker) = fence_marker(line) {
            fence = Some(marker);
            index += 1;
            continue;
        }
        if let Some((level, title)) = heading(line) {
            if is_reference_heading(&title) {
                reference_level = Some(level);
                note_level = None;
                sync_lines.push(line.to_owned());
                index += 1;
                continue;
            }
            if is_note_heading(&title) {
                note_level = Some(level);
                reference_level = None;
                sync_lines.push(line.to_owned());
                index += 1;
                continue;
            }
            if reference_level.is_some_and(|current| level <= current) {
                reference_level = None;
            }
            if note_level.is_some_and(|current| level <= current) {
                note_level = None;
            }
        }
        if let Some(capture) = note_definition.captures(line) {
            let marker = normalize_note_marker(&capture[1]);
            let mut contents = vec![capture[2].to_owned()];
            index += 1;
            while index < lines.len()
                && (lines[index].starts_with(' ') || lines[index].starts_with('\t'))
            {
                contents.push(lines[index].trim().to_owned());
                index += 1;
            }
            let contents = contents.join(" ").trim().to_owned();
            if !omit_prefixes
                .iter()
                .any(|prefix| contents.starts_with(prefix))
            {
                sync_lines.push(contents.clone());
                citation_lines.push(contents.clone());
            }
            notes.insert(marker, contents);
            continue;
        }
        if note_level.is_some() {
            if let Some(capture) = japanese_note_definition.captures(line) {
                let marker = normalize_note_marker(&capture[1]);
                let mut contents = vec![capture[2].to_owned()];
                let mut raw_lines = vec![line.to_owned()];
                index += 1;
                while index < lines.len()
                    && (lines[index].starts_with(' ') || lines[index].starts_with('\t'))
                {
                    contents.push(lines[index].trim().to_owned());
                    raw_lines.push(lines[index].to_owned());
                    index += 1;
                }
                let contents = contents.join(" ").trim().to_owned();
                if !omit_prefixes
                    .iter()
                    .any(|prefix| contents.starts_with(prefix))
                {
                    sync_lines.extend(raw_lines);
                    citation_lines.push(contents.clone());
                }
                notes.insert(marker, contents);
                continue;
            }
            if !omit_prefixes
                .iter()
                .any(|prefix| trimmed.starts_with(prefix))
            {
                sync_lines.push(line.to_owned());
            }
            index += 1;
            continue;
        }
        let omitted = omit_prefixes
            .iter()
            .any(|prefix| trimmed.starts_with(prefix));
        if reference_level.is_some() {
            if !omitted {
                reference_lines.push(line.to_owned());
                sync_lines.push(line.to_owned());
            }
            index += 1;
            continue;
        }
        if !omitted {
            citation_lines.push(line.to_owned());
            if !trimmed.starts_with('|') {
                for capture in note_reference.captures_iter(line) {
                    *note_references
                        .entry(normalize_note_marker(&capture[1]))
                        .or_default() += 1;
                }
                for capture in japanese_note_reference.captures_iter(line) {
                    *note_references
                        .entry(normalize_note_marker(&capture[1]))
                        .or_default() += 1;
                }
                main_lines.push(line.to_owned());
            }
            sync_lines.push(line.to_owned());
        }
        index += 1;
    }
    let main_markdown = main_lines.join("\n");
    SourceView {
        main_text: plain_markdown(&main_markdown),
        citation_text: citation_markdown(&citation_lines.join("\n")),
        sync_text: plain_markdown(&sync_lines.join("\n")),
        prose_paragraphs: prose_blocks(&main_markdown),
        references: reference_entries(&reference_lines),
        notes,
        note_references,
        main_markdown,
    }
}
