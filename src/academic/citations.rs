//! 引用と参考文献の対応、注の扱いの監査。

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use super::source::{SourceView, normalize_note_marker, normalized};
use super::{AcademicCheck, NoteDecision, add_check};

fn normalized_author(author: &str) -> String {
    let author = author
        .rsplit(['。', '！', '？', '!', '?', '\n'])
        .next()
        .unwrap_or_default()
        .replace("et al.", "")
        .replace("et al", "")
        .replace("ほか", "");
    let author = author
        .trim_start_matches(|ch: char| ch.is_ascii_digit() || matches!(ch, '.' | ')' | ']'))
        .trim();
    let author = ["と ", "及び ", "ならびに ", "または "]
        .into_iter()
        .find_map(|prefix| author.strip_prefix(prefix))
        .unwrap_or(author);
    let mixed_latin_tail = Regex::new(r"[A-Za-z][A-Za-z.]*$")
        .expect("valid mixed-script author regex")
        .find(author)
        .filter(|_| {
            author
                .chars()
                .any(|ch| matches!(ch, 'ぁ'..='ん' | 'ァ'..='ヶ' | '一'..='龠'))
        })
        .map(|matched| matched.as_str());
    let primary = author
        .split(['・', '&', ',', '，'])
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default();
    let primary = mixed_latin_tail.unwrap_or(primary);
    primary
        .chars()
        .filter(|ch| ch.is_alphanumeric() || matches!(ch, '々' | 'ヶ' | 'ヵ'))
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn citation_keys(source: &str) -> BTreeSet<String> {
    let narrative = Regex::new(
        r"(?:^|[|。！？、；：\s])(?P<author>[A-Za-zぁ-んァ-ヶー一-龠々ヶヵ・&.,\s]{2,80})\s*[（(]\s*(?P<years>[12][0-9]{3}[a-z]?(?:\s*[、,]\s*[12][0-9]{3}[a-z]?)*?)\s*[）)]",
    )
    .expect("valid narrative citation regex");
    let parenthetical = Regex::new(
        r"(?:^|[|。！？、；：;（(\s])(?P<author>[A-Za-zぁ-んァ-ヶー一-龠々ヶヵ・&.\s]{2,80})\s*[,，]\s*(?P<years>[12][0-9]{3}[a-z]?(?:\s*[、,]\s*[12][0-9]{3}[a-z]?)*?)",
    )
    .expect("valid parenthetical citation regex");
    let years = Regex::new(r"[12][0-9]{3}[a-z]?").expect("valid citation year regex");
    narrative
        .captures_iter(source)
        .chain(parenthetical.captures_iter(source))
        .flat_map(|capture| {
            let author = capture
                .name("author")
                .map(|value| normalized_author(value.as_str()))
                .unwrap_or_default();
            capture
                .name("years")
                .into_iter()
                .flat_map(|value| years.find_iter(value.as_str()))
                .filter(|_| !author.is_empty())
                .map(|year| format!("{}:{}", author, year.as_str().to_ascii_lowercase()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn bracketed_citation_ids(source: &str) -> Vec<String> {
    let bracket =
        Regex::new(r"\\?\[([^\]\r\n]*@[^\]\r\n]+)\\?\]").expect("valid bracketed citation regex");
    let citation =
        Regex::new(r"@([A-Za-z0-9][A-Za-z0-9_.:/+\-]*)").expect("valid citation id regex");
    let mut ids = Vec::new();
    for group in bracket.captures_iter(source) {
        ids.extend(
            citation
                .captures_iter(&group[1])
                .map(|capture| capture[1].to_owned()),
        );
    }
    ids
}

pub(super) fn explicit_citation_ids(source: &str) -> BTreeSet<String> {
    bracketed_citation_ids(source).into_iter().collect()
}

fn explicit_reference_ids(entries: &[String]) -> BTreeMap<String, usize> {
    let mut ids = BTreeMap::new();
    for entry in entries {
        for id in bracketed_citation_ids(entry) {
            *ids.entry(id).or_default() += 1;
        }
    }
    ids
}

fn reference_keys(entry: &str) -> Vec<String> {
    let year = Regex::new(r"[（(]?([12][0-9]{3}[a-z]?)[）)]?").expect("valid reference year regex");
    let identifier = Regex::new(r"(?i)(?:\bdoi\s*:|\bhttps?://)").expect("valid DOI or URL regex");
    let publication = identifier
        .find(entry)
        .map_or(entry, |matched| &entry[..matched.start()]);
    year.captures(publication)
        .and_then(|capture| {
            let full = capture.get(0)?;
            let author = normalized_author(&publication[..full.start()]);
            let year = capture.get(1)?.as_str().to_ascii_lowercase();
            (!author.is_empty()).then_some(format!("{author}:{year}"))
        })
        .into_iter()
        .collect()
}

fn citation_key_matches(
    citation: &str,
    reference: &str,
    aliases: &BTreeMap<String, String>,
) -> bool {
    let Some((citation_author, citation_year)) = citation.split_once(':') else {
        return false;
    };
    let Some((reference_author, reference_year)) = reference.split_once(':') else {
        return false;
    };
    let citation_author = aliases
        .get(citation_author)
        .map_or(citation_author, String::as_str);
    let reference_author = aliases
        .get(reference_author)
        .map_or(reference_author, String::as_str);
    citation_year == reference_year
        && (citation_author == reference_author
            || (citation_author
                .chars()
                .all(|ch| matches!(ch, '一'..='龠' | '々' | 'ヶ' | 'ヵ'))
                && reference_author
                    .chars()
                    .all(|ch| matches!(ch, '一'..='龠' | '々' | 'ヶ' | 'ヵ'))
                && (citation_author.starts_with(reference_author)
                    || reference_author.starts_with(citation_author))))
}

pub(super) fn audit_citations(
    checks: &mut Vec<AcademicCheck>,
    view: &SourceView,
    aliases: &BTreeMap<String, String>,
) {
    let cited_ids = explicit_citation_ids(&view.citation_text);
    let reference_ids = explicit_reference_ids(&view.references);
    let explicit_id_mode = !cited_ids.is_empty() || !reference_ids.is_empty();

    let cited = if explicit_id_mode {
        BTreeSet::new()
    } else {
        citation_keys(&view.citation_text)
    };
    let mut references = BTreeMap::<String, usize>::new();
    if !explicit_id_mode {
        for entry in &view.references {
            for key in reference_keys(entry) {
                *references.entry(key).or_default() += 1;
            }
        }
    }
    let referenced = references.keys().cloned().collect::<BTreeSet<_>>();
    let mut missing = cited
        .iter()
        .filter(|citation| {
            !referenced
                .iter()
                .any(|reference| citation_key_matches(citation, reference, aliases))
        })
        .cloned()
        .collect::<Vec<_>>();
    missing.extend(
        cited_ids
            .iter()
            .filter(|citation| !reference_ids.contains_key(*citation))
            .map(|citation| format!("@{citation}")),
    );
    let mut uncited = referenced
        .iter()
        .filter(|reference| {
            !cited
                .iter()
                .any(|citation| citation_key_matches(citation, reference, aliases))
        })
        .cloned()
        .collect::<Vec<_>>();
    uncited.extend(
        reference_ids
            .keys()
            .filter(|reference| !cited_ids.contains(*reference))
            .map(|reference| format!("@{reference}")),
    );
    let mut duplicate = references
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    duplicate.extend(
        reference_ids
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(key, _)| format!("@{key}")),
    );
    add_check(
        checks,
        "citation_reference_match",
        missing.is_empty(),
        if missing.is_empty() {
            "本文引用に対応する著者名・年が参考文献欄にあります".to_owned()
        } else {
            format!("参考文献欄で対応を確認できません: {}", missing.join(", "))
        },
    );
    add_check(
        checks,
        "uncited_references",
        uncited.is_empty(),
        if uncited.is_empty() {
            "参考文献欄に未引用の著者・年はありません".to_owned()
        } else {
            format!("未引用の参考文献キー: {}", uncited.join(", "))
        },
    );
    add_check(
        checks,
        "duplicate_references",
        duplicate.is_empty(),
        if duplicate.is_empty() {
            "参考文献キーの重複はありません".to_owned()
        } else {
            format!("重複した参考文献キー: {}", duplicate.join(", "))
        },
    );
}

pub(super) fn audit_notes(
    checks: &mut Vec<AcademicCheck>,
    view: &SourceView,
    decisions: &[NoteDecision],
) {
    for decision in decisions {
        let marker = normalize_note_marker(&decision.marker);
        let passed = match decision.disposition.as_str() {
            "note" => view.notes.get(&marker).is_some_and(|text| {
                text.contains(&decision.text)
                    && view.note_references.contains_key(&marker)
                    && !normalized(&view.main_text).contains(&normalized(text))
            }),
            "body" => {
                view.main_text.contains(&decision.text)
                    && !view
                        .notes
                        .values()
                        .any(|text| text.contains(&decision.text))
            }
            "drop" => {
                !view.main_text.contains(&decision.text)
                    && !view
                        .notes
                        .values()
                        .any(|text| text.contains(&decision.text))
            }
            _ => false,
        };
        add_check(
            checks,
            "note_disposition",
            passed,
            format!("{} -> {}", decision.marker, decision.disposition),
        );
    }
    let decided = decisions
        .iter()
        .map(|item| normalize_note_marker(&item.marker))
        .collect::<BTreeSet<_>>();
    let undecided = view
        .notes
        .keys()
        .filter(|marker| !decided.contains(*marker))
        .cloned()
        .collect::<Vec<_>>();
    add_check(
        checks,
        "note_coverage",
        undecided.is_empty(),
        if undecided.is_empty() {
            "全注に本文・注・不要の判定があります".to_owned()
        } else {
            format!("判定のない注: {}", undecided.join(", "))
        },
    );
    let undefined = view
        .note_references
        .keys()
        .filter(|marker| !view.notes.contains_key(*marker))
        .cloned()
        .collect::<Vec<_>>();
    let unused = view
        .notes
        .keys()
        .filter(|marker| !view.note_references.contains_key(*marker))
        .cloned()
        .collect::<Vec<_>>();
    add_check(
        checks,
        "note_references",
        undefined.is_empty() && unused.is_empty(),
        if undefined.is_empty() && unused.is_empty() {
            "注参照と注定義は対応しています".to_owned()
        } else {
            format!(
                "未定義参照: {}; 未使用定義: {}",
                undefined.join(", "),
                unused.join(", ")
            )
        },
    );
}
