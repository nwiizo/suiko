use std::collections::HashMap;

use regex::Regex;
use serde::Serialize;

use crate::Error;
use crate::morphology::{Morpheme, Morphology};
use crate::text::{mask_html_comments, mask_markdown_structure_preserving_headings};

const GLOSS_MARKERS: &[&str] = &["とは", "と呼ぶ", "という", "、つまり"];

#[derive(Clone, Debug, Serialize)]
pub struct Term {
    pub term: String,
    pub first_line: usize,
    pub count: usize,
    pub has_gloss_hint: bool,
    pub context: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TermsReport {
    pub terms: Vec<Term>,
}

#[derive(Clone, Debug)]
struct SeenTerm {
    term: String,
    line: usize,
    byte_offset: usize,
}

fn katakana(surface: &str) -> bool {
    !surface.is_empty()
        && surface
            .chars()
            .all(|ch| matches!(ch, 'ァ'..='ヶ' | 'ー' | '・'))
}

fn capitalized_latin(surface: &str) -> bool {
    let mut chars = surface.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_uppercase())
        && chars.all(|ch| ch.is_ascii_alphanumeric())
        && surface.len() >= 2
}

fn proper_noun(token: &Morpheme) -> bool {
    if token.surface.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return capitalized_latin(&token.surface);
    }
    token.pos(0) == "名詞" && token.pos(1) == "固有名詞" && token.surface.chars().count() >= 2
}

fn register(seen: &mut HashMap<String, SeenTerm>, term: &str, line: usize, offset: usize) {
    let term = term.trim();
    if term.is_empty() {
        return;
    }
    seen.entry(term.to_owned()).or_insert_with(|| SeenTerm {
        term: term.to_owned(),
        line,
        byte_offset: offset,
    });
}

fn context_and_gloss(term: &str, line_no: usize, text: &str) -> (String, bool) {
    const CONTEXT_CHARS: usize = 80;

    let mut line_start = 0;
    let mut line = "";
    for (index, candidate) in text.split('\n').enumerate() {
        if index + 1 == line_no {
            line = candidate;
            break;
        }
        line_start += candidate.len() + 1;
    }
    let Some(byte_start) = line.find(term) else {
        return (
            line.trim().to_owned(),
            GLOSS_MARKERS.iter().any(|marker| line.contains(marker)),
        );
    };
    let absolute_start = line_start + byte_start;
    let absolute_end = absolute_start + term.len();
    let context_start = text[..absolute_start]
        .char_indices()
        .rev()
        .nth(CONTEXT_CHARS.saturating_sub(1))
        .map_or(0, |(byte, _)| byte);
    let context_end = text[absolute_end..]
        .char_indices()
        .nth(CONTEXT_CHARS)
        .map_or(text.len(), |(byte, _)| absolute_end + byte);
    let context = &text[context_start..context_end];
    let after = &text[absolute_end..context_end];
    let hint = after.starts_with('(')
        || after.starts_with('（')
        || GLOSS_MARKERS.iter().any(|marker| context.contains(marker));
    (context.trim().to_owned(), hint)
}

pub fn analyze(raw_text: &str, morphology: &Morphology) -> Result<TermsReport, Error> {
    let comments_masked = mask_html_comments(raw_text);
    let body_masked = mask_markdown_structure_preserving_headings(&comments_masked);
    let lines = body_masked
        .split('\n')
        .enumerate()
        .map(|(index, line)| (index + 1, line.to_owned()))
        .collect::<Vec<_>>();

    let acronym = Regex::new(r"[A-Z]{2,}[0-9]*").expect("valid acronym regex");
    let mut seen = HashMap::new();
    for (line_no, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        for found in acronym.find_iter(&line) {
            let before_ok = line[..found.start()]
                .chars()
                .next_back()
                .is_none_or(|ch| !ch.is_ascii_alphanumeric());
            let after_ok = line[found.end()..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_alphanumeric());
            if before_ok && after_ok {
                register(&mut seen, found.as_str(), line_no, found.start());
            }
        }

        let tokens = morphology.tokenize(&line)?;
        let mut index = 0;
        while index < tokens.len() {
            let predicate: Option<fn(&Morpheme) -> bool> = if katakana(&tokens[index].surface) {
                Some(|token| katakana(&token.surface))
            } else if proper_noun(&tokens[index]) {
                Some(proper_noun)
            } else {
                None
            };
            let Some(predicate) = predicate else {
                index += 1;
                continue;
            };
            let mut end = index + 1;
            while end < tokens.len()
                && predicate(&tokens[end])
                && tokens[end - 1].byte_end == tokens[end].byte_start
            {
                end += 1;
            }
            let byte_start = tokens[index].byte_start;
            let byte_end = tokens[end - 1].byte_end;
            let raw_term = &line[byte_start..byte_end];
            let term = raw_term.trim_matches('・');
            if !katakana(&tokens[index].surface) || term.chars().count() >= 3 {
                let trimmed_start = byte_start + raw_term.find(term).unwrap_or_default();
                register(&mut seen, term, line_no, trimmed_start);
            }
            index = end;
        }
    }

    let mut seen = seen.into_values().collect::<Vec<_>>();
    seen.sort_by_key(|term| (term.line, term.byte_offset));
    let terms = seen
        .into_iter()
        .map(|seen| {
            let (context, has_gloss_hint) =
                context_and_gloss(&seen.term, seen.line, &comments_masked);
            Term {
                count: comments_masked.match_indices(&seen.term).count(),
                term: seen.term,
                first_line: seen.line,
                has_gloss_hint,
                context,
            }
        })
        .collect();
    Ok(TermsReport { terms })
}
