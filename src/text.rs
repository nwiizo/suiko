use regex::Regex;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sentence {
    pub line: usize,
    pub text: String,
    pub raw_text: String,
}

pub fn numbered_lines(text: &str) -> Vec<(usize, &str)> {
    text.split('\n')
        .enumerate()
        .map(|(i, line)| (i + 1, line))
        .collect()
}

pub fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && trimmed
            .as_bytes()
            .get(hashes)
            .is_none_or(u8::is_ascii_whitespace)
}

pub fn heading(line: &str) -> Option<(usize, String)> {
    if !is_heading(line) {
        return None;
    }
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
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

pub fn mask_markdown_structure(text: &str) -> String {
    mask_markdown(text, false)
}

pub fn mask_markdown_structure_preserving_headings(text: &str) -> String {
    mask_markdown(text, true)
}

fn mask_markdown(text: &str, preserve_headings: bool) -> String {
    let text = mask_html_comments(text);
    let inline_code = Regex::new(r"``[^\n]*?``|`[^`\n]+`").expect("valid inline-code regex");
    let inline_html =
        Regex::new(r"</?[A-Za-z][A-Za-z0-9-]*(?:\s[^>\n]*)?/?>").expect("valid HTML-tag regex");
    let link_url = Regex::new(r"(\]\()([^)]*)(\))").expect("valid Markdown-link regex");
    let embed_citation =
        Regex::new(r"^\[https?://[^]\n]+:embed:cite\]$").expect("valid embed-citation regex");
    let mut masked = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut front_matter = false;

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

        let no_code = inline_code.replace_all(line, |captures: &regex::Captures<'_>| {
            " ".repeat(captures[0].len())
        });
        let no_html = inline_html.replace_all(&no_code, |captures: &regex::Captures<'_>| {
            " ".repeat(captures[0].len())
        });
        let no_urls = link_url.replace_all(&no_html, |captures: &regex::Captures<'_>| {
            format!(
                "{}{}{}",
                &captures[1],
                " ".repeat(captures[2].len()),
                &captures[3]
            )
        });
        masked.push(no_urls.into_owned());
    }
    masked.join("\n")
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
                let sentence = line[start..end].trim();
                if !sentence.is_empty() {
                    output.push(Sentence {
                        line: line_no,
                        text: sentence.to_owned(),
                        raw_text: raw_line
                            .get(start..end)
                            .unwrap_or(sentence)
                            .trim()
                            .to_owned(),
                    });
                }
                start = byte + ch.len_utf8();
            }
        }
        let tail = line[start..].trim();
        if !tail.is_empty() {
            output.push(Sentence {
                line: line_no,
                text: tail.to_owned(),
                raw_text: raw_line.get(start..).unwrap_or(tail).trim().to_owned(),
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
