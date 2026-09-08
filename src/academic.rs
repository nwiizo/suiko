use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::escape::resolve_xml_entity;
use quick_xml::events::Event;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::outline::{OutlineEntry, build_outline};
use crate::text::{heading, mask_html_comments};
use crate::{Error, read_source};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcademicContract {
    pub version: u32,
    pub central_claim: String,
    pub explanandum: String,
    pub allow_formal_research_question: bool,
    pub reject_defensive_caveats: bool,
    pub required_order: Vec<String>,
    pub terms: Vec<TermContract>,
    pub proper_nouns: Vec<String>,
    /// 引用符付きで用いることを契約上確認済みの分類名、公式値、短いラベル。
    #[serde(default)]
    pub accepted_labels: Vec<String>,
    /// 同一著者を異なる文字体系で記す場合の正規化（例: 井庭 -> iba）。
    #[serde(default)]
    pub citation_aliases: BTreeMap<String, String>,
    /// 段落第一文が進めるべき論旨を、本文に出る順で記録する。
    pub first_sentence_order: Vec<String>,
    pub section_bridges: Vec<SectionBridge>,
    pub note_decisions: Vec<NoteDecision>,
    pub style_profile: StyleProfile,
    #[serde(default)]
    pub sync_omit_prefixes: Vec<String>,
    /// 版型固有のヘッダー等、三成果物の同期から意図して除外する文字列。
    #[serde(default)]
    pub sync_omit_fragments: Vec<String>,
    #[serde(default = "default_mutable_entries")]
    pub docx_mutable_entries: Vec<String>,
    /// DOCX本文で確認する見出し・表題・図題等の出現順。
    #[serde(default)]
    pub docx_layout_order: Vec<String>,
}

fn default_mutable_entries() -> Vec<String> {
    vec![
        "docProps/core.xml".to_owned(),
        "docProps/app.xml".to_owned(),
    ]
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermContract {
    pub term: String,
    pub status: String,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionBridge {
    pub from: String,
    pub to: String,
    pub shared_terms: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDecision {
    pub marker: String,
    pub disposition: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StyleProfile {
    pub sentence_length_mean_min: f64,
    pub sentence_length_mean_max: f64,
    pub citation_led_paragraph_ratio_max: f64,
    #[serde(default)]
    pub connectives: Vec<String>,
    #[serde(default)]
    pub discovery_marker_at_paragraph_end: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRecord {
    pub exporter: String,
    pub pdf_visual_reviewed: bool,
    pub source_sha256: String,
    pub docx_sha256: String,
    pub pdf_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcademicCheck {
    pub id: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct StyleObservations {
    pub sentence_length_mean: f64,
    pub paragraph_count: usize,
    pub citation_led_paragraph_ratio: f64,
    pub connective_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcademicReport {
    pub passed: bool,
    pub delivery_ready: bool,
    pub checks: Vec<AcademicCheck>,
    pub first_sentences: Vec<OutlineEntry>,
    pub style: StyleObservations,
}

pub struct ArtifactPaths<'a> {
    pub source: &'a Path,
    pub docx: Option<&'a Path>,
    pub pdf: Option<&'a Path>,
    pub template: Option<&'a Path>,
    pub export_record: Option<&'a Path>,
}

fn add_check(checks: &mut Vec<AcademicCheck>, id: &str, passed: bool, detail: impl Into<String>) {
    checks.push(AcademicCheck {
        id: id.to_owned(),
        status: if passed { "pass" } else { "fail" }.to_owned(),
        detail: detail.into(),
    });
}

fn add_review(checks: &mut Vec<AcademicCheck>, id: &str, detail: impl Into<String>) {
    checks.push(AcademicCheck {
        id: id.to_owned(),
        status: "review".to_owned(),
        detail: detail.into(),
    });
}

fn has_coined_term_provenance(context: &str) -> bool {
    context.contains("造語")
        || ((context.contains("本稿")
            || context.contains("本研究")
            || context.contains("本研究プロジェクト"))
            && (context.contains("と呼ぶ")
                || context.contains("と定義")
                || context.contains("分析用に定めた")
                || context.contains("操作的分類")))
}

fn validate_contract(contract: &AcademicContract) -> Result<(), Error> {
    if contract.version != 1 {
        return Err(Error::Academic(format!(
            "contract version {} は未対応です",
            contract.version
        )));
    }
    if contract.central_claim.trim().is_empty() || contract.explanandum.trim().is_empty() {
        return Err(Error::Academic(
            "central_claim と explanandum は空にできません".to_owned(),
        ));
    }
    if contract.required_order.is_empty() || contract.first_sentence_order.is_empty() {
        return Err(Error::Academic(
            "required_order と first_sentence_order は少なくとも1項目必要です".to_owned(),
        ));
    }
    if contract.terms.is_empty() {
        return Err(Error::Academic(
            "terms は空にできません。普通名詞も ordinary として明示してください".to_owned(),
        ));
    }
    for term in &contract.terms {
        if !matches!(
            term.status.as_str(),
            "established" | "source-specific" | "coined" | "ordinary"
        ) {
            return Err(Error::Academic(format!(
                "未知の用語statusです: {}",
                term.status
            )));
        }
        if matches!(term.status.as_str(), "established" | "source-specific")
            && term.source.as_deref().is_none_or(str::is_empty)
        {
            return Err(Error::Academic(format!(
                "{} にはsourceが必要です",
                term.term
            )));
        }
    }
    for note in &contract.note_decisions {
        if !matches!(note.disposition.as_str(), "body" | "note" | "drop") {
            return Err(Error::Academic(format!(
                "{} のnote dispositionが不正です",
                note.marker
            )));
        }
    }
    Ok(())
}

fn normalized(text: &str) -> String {
    text.chars()
        .map(|ch| if ch == '：' { ':' } else { ch })
        .filter(|ch| !ch.is_whitespace() && !ch.is_control())
        .filter(|ch| !matches!(ch, '*' | '_' | '`'))
        .collect()
}

#[derive(Debug)]
struct SourceView {
    main_markdown: String,
    main_text: String,
    citation_text: String,
    sync_text: String,
    prose_paragraphs: Vec<String>,
    references: Vec<String>,
    notes: BTreeMap<String, String>,
    note_references: BTreeMap<String, usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct SyncInventory {
    blocks: BTreeMap<String, usize>,
    characters: BTreeMap<char, usize>,
    character_count: usize,
    duplicate_blocks: usize,
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

fn normalize_note_marker(marker: &str) -> String {
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

fn plain_markdown(text: &str) -> String {
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

fn source_view(source: &str, omit_prefixes: &[String]) -> SourceView {
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

fn sentences(text: &str) -> Vec<String> {
    text.split_inclusive(['。', '！', '？'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .map(str::to_owned)
        .collect()
}

fn style_observations(view: &SourceView, profile: &StyleProfile) -> StyleObservations {
    let paragraphs = &view.prose_paragraphs;
    let style_text = paragraphs.join("\n");
    let sentence_list = sentences(&style_text);
    let mean = if sentence_list.is_empty() {
        0.0
    } else {
        sentence_list
            .iter()
            .map(|sentence| sentence.chars().count() as f64)
            .sum::<f64>()
            / sentence_list.len() as f64
    };
    let citation_lead = Regex::new(r"^[A-Za-z一-龠々・]{2,24}[（(][12][0-9]{3}[）)]")
        .expect("valid citation-lead regex");
    let citation_led = paragraphs
        .iter()
        .filter(|paragraph| citation_lead.is_match(paragraph))
        .count();
    let ratio = if paragraphs.is_empty() {
        0.0
    } else {
        citation_led as f64 / paragraphs.len() as f64
    };
    let connective_counts = profile
        .connectives
        .iter()
        .map(|word| (word.clone(), style_text.matches(word).count()))
        .collect();
    StyleObservations {
        sentence_length_mean: (mean * 100.0).round() / 100.0,
        paragraph_count: paragraphs.len(),
        citation_led_paragraph_ratio: (ratio * 1000.0).round() / 1000.0,
        connective_counts,
    }
}

fn section_bounds(outline: &[OutlineEntry], heading: &str) -> Option<(usize, usize)> {
    let start = outline
        .iter()
        .position(|entry| entry.kind == "heading" && entry.text == heading)?;
    let level = outline[start].level.unwrap_or(1);
    let end = outline[start + 1..]
        .iter()
        .position(|entry| entry.kind == "heading" && entry.level.unwrap_or(1) <= level)
        .map_or(outline.len(), |offset| start + 1 + offset);
    Some((start, end))
}

fn academic_outline(view: &SourceView) -> Vec<OutlineEntry> {
    build_outline(&view.main_markdown)
        .into_iter()
        .filter(|entry| {
            !(entry.kind == "lead"
                && (entry.text.starts_with("[^") || plain_markdown(&entry.text).trim().is_empty()))
        })
        .collect()
}

fn audit_bridges(
    checks: &mut Vec<AcademicCheck>,
    outline: &[OutlineEntry],
    rules: &[SectionBridge],
) {
    for rule in rules {
        let Some((from_start, from_end)) = section_bounds(outline, &rule.from) else {
            add_check(
                checks,
                "section_bridge",
                false,
                format!("見出しがありません: {}", rule.from),
            );
            continue;
        };
        let Some((to_start, to_end)) = section_bounds(outline, &rule.to) else {
            add_check(
                checks,
                "section_bridge",
                false,
                format!("見出しがありません: {}", rule.to),
            );
            continue;
        };
        let from = outline[from_start..from_end]
            .iter()
            .rev()
            .find(|entry| entry.kind == "lead")
            .map(|entry| entry.text.as_str())
            .unwrap_or_default();
        let to = outline[to_start..to_end]
            .iter()
            .find(|entry| entry.kind == "lead")
            .map(|entry| entry.text.as_str())
            .unwrap_or_default();
        let shared = rule
            .shared_terms
            .iter()
            .any(|term| from.contains(term) && to.contains(term));
        add_check(
            checks,
            "section_bridge",
            shared,
            format!(
                "{} -> {}: 前節末「{}」 / 次節冒頭「{}」",
                rule.from, rule.to, from, to
            ),
        );
    }
}

fn audit_first_sentence_order(
    checks: &mut Vec<AcademicCheck>,
    outline: &[OutlineEntry],
    required: &[String],
) {
    let leads = outline
        .iter()
        .filter(|entry| entry.kind == "lead")
        .map(|entry| entry.text.as_str())
        .collect::<Vec<_>>();
    let mut next = 0;
    let mut missing = None;
    for required_text in required {
        let Some(offset) = leads[next..]
            .iter()
            .position(|lead| lead.contains(required_text))
        else {
            missing = Some(required_text);
            break;
        };
        next += offset + 1;
    }
    add_check(
        checks,
        "first_sentence_order",
        missing.is_none(),
        missing.map_or_else(
            || "段落第一文の論旨順を確認".to_owned(),
            |text| format!("段落第一文にない、又は順序が崩れた命題: {text}"),
        ),
    );
}

fn audit_proper_nouns(checks: &mut Vec<AcademicCheck>, source: &str, known: &[String]) {
    let citation = Regex::new(r"([A-Za-z一-龠々・]{2,30})[（(][12][0-9]{3}[a-z]?[）)]")
        .expect("valid citation regex");
    let unknown = citation
        .captures_iter(source)
        .map(|capture| capture[1].to_owned())
        .filter(|name| !known.iter().any(|known_name| known_name == name))
        .collect::<BTreeSet<_>>();
    if !unknown.is_empty() {
        add_review(
            checks,
            "unregistered_proper_noun",
            format!(
                "契約にない本文引用の固有名詞候補: {}",
                unknown.into_iter().collect::<Vec<_>>().join(", ")
            ),
        );
    }
}

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

fn citation_keys(source: &str) -> BTreeSet<String> {
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

fn explicit_citation_ids(source: &str) -> BTreeSet<String> {
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

fn audit_citations(
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

fn audit_notes(checks: &mut Vec<AcademicCheck>, view: &SourceView, decisions: &[NoteDecision]) {
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

fn zip_entries(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let file = File::open(path)
        .map_err(|error| Error::Academic(format!("{}を開けません: {error}", path.display())))?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        Error::Academic(format!(
            "{}は有効なDOCXではありません: {error}",
            path.display()
        ))
    })?;
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| Error::Academic(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| Error::Academic(error.to_string()))?;
        entries.insert(name, bytes);
    }
    Ok(entries)
}

fn word_xml_text(xml: &[u8]) -> Result<String, Error> {
    let mut reader = Reader::from_reader(xml);
    let mut output = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(text)) => {
                let decoded = text
                    .decode()
                    .map_err(|error| Error::Academic(error.to_string()))?;
                output.push_str(&decoded);
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(character) = reference
                    .resolve_char_ref()
                    .map_err(|error| Error::Academic(error.to_string()))?
                {
                    output.push(character);
                } else {
                    let name = reference
                        .decode()
                        .map_err(|error| Error::Academic(error.to_string()))?;
                    if let Some(value) = resolve_xml_entity(&name) {
                        output.push_str(value);
                    }
                }
            }
            Ok(Event::End(end)) if end.name().as_ref() == b"w:p" => output.push('\n'),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(Error::Academic(error.to_string())),
        }
    }
    Ok(output)
}

fn docx_text(path: &Path) -> Result<String, Error> {
    let entries = zip_entries(path)?;
    let document = entries
        .get("word/document.xml")
        .ok_or_else(|| Error::Academic("DOCXにword/document.xmlがありません".to_owned()))?;
    let mut parts = vec![word_xml_text(document)?];
    for name in ["word/footnotes.xml", "word/endnotes.xml"] {
        if let Some(xml) = entries.get(name) {
            parts.push(word_xml_text(xml)?);
        }
    }
    Ok(parts.join("\n"))
}

#[derive(Debug, PartialEq, Eq)]
enum DocxLayoutItem {
    Paragraph(String),
    Table,
    Figure,
}

fn docx_layout_items(path: &Path) -> Result<Vec<DocxLayoutItem>, Error> {
    let entries = zip_entries(path)?;
    let document = entries
        .get("word/document.xml")
        .ok_or_else(|| Error::Academic("DOCXにword/document.xmlがありません".to_owned()))?;
    let mut reader = Reader::from_reader(document.as_slice());
    let mut items = Vec::new();
    let mut table_depth = 0usize;
    let mut in_paragraph = false;
    let mut paragraph_text = String::new();
    let mut paragraph_has_figure = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => match start.name().as_ref() {
                b"w:tbl" => {
                    if table_depth == 0 {
                        items.push(DocxLayoutItem::Table);
                    }
                    table_depth += 1;
                }
                b"w:p" if table_depth == 0 => {
                    in_paragraph = true;
                    paragraph_text.clear();
                    paragraph_has_figure = false;
                }
                b"w:drawing" if in_paragraph && table_depth == 0 => {
                    paragraph_has_figure = true;
                }
                _ => {}
            },
            Ok(Event::Empty(empty)) => match empty.name().as_ref() {
                b"w:drawing" if in_paragraph && table_depth == 0 => {
                    paragraph_has_figure = true;
                }
                _ => {}
            },
            Ok(Event::Text(text)) if in_paragraph && table_depth == 0 => {
                let decoded = text
                    .decode()
                    .map_err(|error| Error::Academic(error.to_string()))?;
                paragraph_text.push_str(&decoded);
            }
            Ok(Event::GeneralRef(reference)) if in_paragraph && table_depth == 0 => {
                if let Some(character) = reference
                    .resolve_char_ref()
                    .map_err(|error| Error::Academic(error.to_string()))?
                {
                    paragraph_text.push(character);
                } else {
                    let name = reference
                        .decode()
                        .map_err(|error| Error::Academic(error.to_string()))?;
                    if let Some(value) = resolve_xml_entity(&name) {
                        paragraph_text.push_str(value);
                    }
                }
            }
            Ok(Event::End(end)) => match end.name().as_ref() {
                b"w:p" if in_paragraph && table_depth == 0 => {
                    if paragraph_has_figure {
                        items.push(DocxLayoutItem::Figure);
                    }
                    let text = paragraph_text.trim();
                    if !text.is_empty() {
                        items.push(DocxLayoutItem::Paragraph(text.to_owned()));
                    }
                    in_paragraph = false;
                }
                b"w:tbl" => table_depth = table_depth.saturating_sub(1),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(Error::Academic(error.to_string())),
        }
    }
    Ok(items)
}

fn audit_docx_layout(
    checks: &mut Vec<AcademicCheck>,
    docx: &Path,
    required_order: &[String],
) -> Result<(), Error> {
    let items = docx_layout_items(docx)?;
    let caption = Regex::new(r"^(表|図)[-－ー]?[0-9０-９]+\s+").expect("valid caption regex");
    let mut table_captions = 0usize;
    let mut figure_captions = 0usize;
    let mut misplaced = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let DocxLayoutItem::Paragraph(text) = item else {
            continue;
        };
        let Some(kind) = caption
            .captures(text.trim_start())
            .map(|capture| capture[1].to_owned())
        else {
            continue;
        };
        let expected = if kind == "表" {
            table_captions += 1;
            DocxLayoutItem::Table
        } else {
            figure_captions += 1;
            DocxLayoutItem::Figure
        };
        let paired = index
            .checked_sub(1)
            .and_then(|previous| items.get(previous))
            .is_some_and(|item| item == &expected);
        if !paired {
            misplaced.push(text.clone());
        }
    }
    let tables = items
        .iter()
        .filter(|item| matches!(item, DocxLayoutItem::Table))
        .count();
    let figures = items
        .iter()
        .filter(|item| matches!(item, DocxLayoutItem::Figure))
        .count();
    let pairs_ok = misplaced.is_empty() && tables == table_captions && figures == figure_captions;
    add_check(
        checks,
        "docx_table_figure_placement",
        pairs_ok,
        if pairs_ok {
            format!("表{tables}点・図{figures}点の直後に対応する表題・図題があります")
        } else {
            format!(
                "表/表題 {tables}/{table_captions}、図/図題 {figures}/{figure_captions}、位置不整合: {}",
                misplaced.join(", ")
            )
        },
    );

    let paragraphs = items
        .iter()
        .filter_map(|item| match item {
            DocxLayoutItem::Paragraph(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut missing = Vec::new();
    for anchor in required_order {
        if let Some(offset) = paragraphs[cursor..]
            .iter()
            .position(|paragraph| paragraph.contains(anchor))
        {
            cursor += offset + 1;
        } else {
            missing.push(anchor.clone());
        }
    }
    add_check(
        checks,
        "docx_layout_order",
        missing.is_empty(),
        if missing.is_empty() {
            format!(
                "指定した{}個の配置基準を順に確認しました",
                required_order.len()
            )
        } else {
            format!("指定位置以降で確認できない配置基準: {}", missing.join(", "))
        },
    );
    Ok(())
}

fn protected_document_layout(xml: &[u8]) -> Result<Vec<String>, Error> {
    let xml = std::str::from_utf8(xml).map_err(|error| {
        Error::Academic(format!("word/document.xmlはUTF-8ではありません: {error}"))
    })?;
    let section = Regex::new(r#"(?s)<w:sectPr\b.*?</w:sectPr>|<w:sectPr\b[^>]*/>"#)
        .expect("valid section regex");
    let protected = Regex::new(
        r#"(?s)<w:type\b[^>]*/>|<w:pgSz\b[^>]*/>|<w:pgMar\b[^>]*/>|<w:cols\b[^>]*/>|<w:docGrid\b[^>]*/>|<w:pgBorders\b.*?</w:pgBorders>|<w:pgBorders\b[^>]*/>|<w:(?:headerReference|footerReference)\b[^>]*/>"#,
    )
    .expect("valid protected section settings regex");
    Ok(section
        .find_iter(xml)
        .flat_map(|section| protected.find_iter(section.as_str()))
        .map(|item| normalized(item.as_str()))
        .collect())
}

fn xml_elements(
    xml: &[u8],
    element_names: &[&str],
    identity_attribute: &str,
) -> Result<BTreeMap<String, String>, Error> {
    let xml = std::str::from_utf8(xml)
        .map_err(|error| Error::Academic(format!("OOXMLはUTF-8ではありません: {error}")))?;
    let elements = Regex::new(&format!(
        r#"<(?:{})\b(?P<attributes>[^>]*)/?>"#,
        element_names.join("|")
    ))
    .expect("valid OOXML element regex");
    let attributes = Regex::new(r#"([A-Za-z:]+)="([^"]*)""#).expect("valid OOXML attribute regex");
    let mut values = BTreeMap::new();
    for element in elements.captures_iter(xml) {
        let mut attributes = attributes
            .captures_iter(
                element
                    .name("attributes")
                    .map_or("", |value| value.as_str()),
            )
            .map(|attribute| (attribute[1].to_owned(), attribute[2].to_owned()))
            .collect::<BTreeMap<_, _>>();
        let identity = attributes.remove(identity_attribute).ok_or_else(|| {
            Error::Academic(format!(
                "OOXMLの{}に{}がありません",
                element_names.join("/"),
                identity_attribute
            ))
        })?;
        let content = attributes
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(";");
        values.insert(identity, content);
    }
    Ok(values)
}

fn preserves_xml_elements(
    before: &[u8],
    after: &[u8],
    element_names: &[&str],
    identity_attribute: &str,
) -> Result<bool, Error> {
    let before = xml_elements(before, element_names, identity_attribute)?;
    let after = xml_elements(after, element_names, identity_attribute)?;
    Ok(before
        .iter()
        .all(|(identity, value)| after.get(identity) == Some(value)))
}

fn preserves_horizontal_vml_lines(before: &[u8], after: &[u8]) -> Result<bool, Error> {
    fn horizontal(xml: &[u8]) -> Result<BTreeMap<String, String>, Error> {
        let lines = xml_elements(xml, &["v:line"], "id")?;
        Ok(lines
            .into_iter()
            .filter(|(_, attributes)| {
                let from = attributes
                    .split(';')
                    .find_map(|item| item.strip_prefix("from="));
                let to = attributes
                    .split(';')
                    .find_map(|item| item.strip_prefix("to="));
                match (from, to) {
                    (Some(from), Some(to)) => {
                        from.split_once(',').map(|(_, y)| y) == to.split_once(',').map(|(_, y)| y)
                    }
                    _ => false,
                }
            })
            .collect())
    }
    let before = horizontal(before)?;
    let after = horizontal(after)?;
    Ok(before
        .iter()
        .all(|(identity, value)| after.get(identity) == Some(value)))
}

fn preserves_content_types(before: &[u8], after: &[u8]) -> Result<bool, Error> {
    Ok(
        preserves_xml_elements(before, after, &["Default"], "Extension")?
            && preserves_xml_elements(before, after, &["Override"], "PartName")?,
    )
}

fn mutable(name: &str, rules: &[String]) -> bool {
    rules.iter().any(|rule| {
        rule.strip_suffix('*')
            .map_or(name == rule, |prefix| name.starts_with(prefix))
    })
}

fn audit_ooxml(
    checks: &mut Vec<AcademicCheck>,
    template: &Path,
    docx: &Path,
    mutable_entries: &[String],
) -> Result<(), Error> {
    let before = zip_entries(template)?;
    let after = zip_entries(docx)?;
    let changed = before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|name| {
            if name.as_str() == "word/document.xml" || mutable(name, mutable_entries) {
                return false;
            }
            match name.as_str() {
                "[Content_Types].xml" => before.get(*name).is_none_or(|before_xml| {
                    after.get(*name).is_none_or(|after_xml| {
                        !preserves_content_types(before_xml, after_xml).unwrap_or(false)
                    })
                }),
                "word/_rels/document.xml.rels" => before.get(*name).is_none_or(|before_xml| {
                    after.get(*name).is_none_or(|after_xml| {
                        !preserves_xml_elements(before_xml, after_xml, &["Relationship"], "Id")
                            .unwrap_or(false)
                    })
                }),
                name if name.starts_with("word/media/") => {
                    before.contains_key(name) && before.get(name) != after.get(name)
                }
                _ => before.get(*name) != after.get(*name),
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    let before_layout = before
        .get("word/document.xml")
        .ok_or_else(|| Error::Academic("テンプレートにword/document.xmlがありません".to_owned()))
        .and_then(|xml| protected_document_layout(xml))?;
    let after_layout = after
        .get("word/document.xml")
        .ok_or_else(|| Error::Academic("DOCXにword/document.xmlがありません".to_owned()))
        .and_then(|xml| protected_document_layout(xml))?;
    let layout_changed = before_layout != after_layout;
    let template_rules_preserved = preserves_horizontal_vml_lines(
        before
            .get("word/document.xml")
            .expect("template document checked above"),
        after
            .get("word/document.xml")
            .expect("submission document checked above"),
    )?;
    add_check(
        checks,
        "ooxml_invariants",
        changed.is_empty() && !layout_changed && template_rules_preserved,
        if changed.is_empty() && !layout_changed && template_rules_preserved {
            "許可していないテンプレートOOXML、節設定及び第一頁罫線は不変です".to_owned()
        } else {
            format!(
                "許可なく変わったOOXML: {}; 節設定変更: {}; 第一頁罫線維持: {}",
                changed.join(", "),
                if layout_changed { "あり" } else { "なし" },
                if template_rules_preserved {
                    "はい"
                } else {
                    "いいえ"
                }
            )
        },
    );
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String, Error> {
    let bytes = std::fs::read(path).map_err(|source| Error::Read {
        path: path.display().to_string(),
        source,
    })?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn audit_export_record(
    checks: &mut Vec<AcademicCheck>,
    record_path: &Path,
    paths: &ArtifactPaths<'_>,
) -> Result<(), Error> {
    let record: ExportRecord = serde_json::from_str(&read_source(record_path)?)?;
    add_check(
        checks,
        "word_exporter_declaration",
        record.exporter == "Microsoft Word",
        format!(
            "納品記録の自己申告exporter: {}（SuikoはWord出力自体を実証しません）",
            record.exporter
        ),
    );
    add_check(
        checks,
        "pdf_visual_review_declaration",
        record.pdf_visual_reviewed,
        "PDF全頁を画像として目視確認した自己申告を確認（実施事実は機械検証しません）",
    );
    let source_ok = file_sha256(paths.source)? == record.source_sha256;
    let docx_ok = paths
        .docx
        .is_some_and(|path| file_sha256(path).is_ok_and(|hash| hash == record.docx_sha256));
    let pdf_ok = paths
        .pdf
        .is_some_and(|path| file_sha256(path).is_ok_and(|hash| hash == record.pdf_sha256));
    add_check(
        checks,
        "export_record_hashes",
        source_ok && docx_ok && pdf_ok,
        "納品記録のMarkdown・DOCX・PDFハッシュを照合しました",
    );
    Ok(())
}

fn sync_normalized(text: &str, omitted_fragments: &[String]) -> String {
    let mut text = normalized(text);
    for fragment in omitted_fragments {
        text = text.replace(&normalized(fragment), "");
    }
    text
}

fn sync_inventory(text: &str, omitted_fragments: &[String]) -> SyncInventory {
    let normalized_text = sync_normalized(text, omitted_fragments);
    let mut blocks = BTreeMap::<String, usize>::new();
    for block in text
        .split_inclusive(['。', '！', '？', '.', '!', '?'])
        .map(|block| sync_normalized(block, omitted_fragments))
        .filter(|block| !block.is_empty())
    {
        *blocks.entry(block).or_default() += 1;
    }
    let mut characters = BTreeMap::<char, usize>::new();
    for character in normalized_text.chars() {
        *characters.entry(character).or_default() += 1;
    }
    let duplicate_blocks = blocks
        .values()
        .filter(|count| **count > 1)
        .map(|count| count - 1)
        .sum();
    SyncInventory {
        blocks,
        characters,
        character_count: normalized_text.chars().count(),
        duplicate_blocks,
    }
}

fn inventories_match(source: &SyncInventory, artifact: &SyncInventory) -> bool {
    source.blocks == artifact.blocks
        && source.characters == artifact.characters
        && source.character_count == artifact.character_count
        && source.duplicate_blocks == artifact.duplicate_blocks
}

fn inventory_difference(source: &SyncInventory, artifact: &SyncInventory) -> String {
    fn character_delta(left: &BTreeMap<char, usize>, right: &BTreeMap<char, usize>) -> Vec<String> {
        left.iter()
            .filter_map(|(character, count)| {
                let difference = count.saturating_sub(*right.get(character).unwrap_or(&0));
                (difference > 0).then(|| format!("{character:?}×{difference}"))
            })
            .take(20)
            .collect()
    }
    let source_only = character_delta(&source.characters, &artifact.characters);
    let artifact_only = character_delta(&artifact.characters, &source.characters);
    format!(
        "Markdownのみ [{}] / 成果物のみ [{}]",
        source_only.join(", "),
        artifact_only.join(", ")
    )
}

fn audit_sync(
    checks: &mut Vec<AcademicCheck>,
    view: &SourceView,
    contract: &AcademicContract,
    paths: &ArtifactPaths<'_>,
) -> Result<(), Error> {
    let source = sync_inventory(&view.sync_text, &contract.sync_omit_fragments);
    if let Some(docx) = paths.docx {
        let text = sync_inventory(&docx_text(docx)?, &contract.sync_omit_fragments);
        let matches = inventories_match(&source, &text);
        add_check(
            checks,
            "markdown_docx_sync",
            matches,
            format!(
                "MarkdownとDOCXの本文・表・注・参考文献を順序非依存で双方向照合（文字数 {} / {}、重複ブロック {} / {}）: {}",
                source.character_count,
                text.character_count,
                source.duplicate_blocks,
                text.duplicate_blocks,
                if matches {
                    "一致".to_owned()
                } else {
                    format!("不一致; {}", inventory_difference(&source, &text))
                }
            ),
        );
    }
    if let Some(pdf) = paths.pdf {
        let extracted = pdf_extract::extract_text(pdf)
            .map_err(|error| Error::Academic(format!("PDF本文を抽出できません: {error}")))?;
        let text = sync_inventory(&extracted, &contract.sync_omit_fragments);
        let matches = inventories_match(&source, &text);
        add_check(
            checks,
            "markdown_pdf_sync",
            matches,
            format!(
                "MarkdownとPDFの本文・表・注・参考文献を順序非依存で双方向照合（文字数 {} / {}、重複ブロック {} / {}）: {}",
                source.character_count,
                text.character_count,
                source.duplicate_blocks,
                text.duplicate_blocks,
                if matches {
                    "一致".to_owned()
                } else {
                    format!("不一致; {}", inventory_difference(&source, &text))
                }
            ),
        );
    }
    Ok(())
}

pub fn audit(
    source: &str,
    contract: &AcademicContract,
    paths: &ArtifactPaths<'_>,
) -> Result<AcademicReport, Error> {
    validate_contract(contract)?;
    let view = source_view(source, &contract.sync_omit_prefixes);
    let main = view.main_text.as_str();
    let mut checks = Vec::new();
    add_check(
        &mut checks,
        "central_claim",
        main.contains(&contract.central_claim),
        "中心命題を本文で確認",
    );
    add_check(
        &mut checks,
        "explanandum",
        main.contains(&contract.explanandum),
        "説明対象を本文で確認",
    );

    let rq =
        Regex::new(r"(?i)リサーチ[・ ]?クエスチョン|\bRQ\s*[0-9０-９]").expect("valid RQ regex");
    add_check(
        &mut checks,
        "research_question_policy",
        contract.allow_formal_research_question || !rq.is_match(main),
        "形式的なリサーチクエスチョンの扱いを確認",
    );

    let mut last = 0;
    let mut order_ok = true;
    for item in &contract.required_order {
        if let Some(offset) = main[last..].find(item) {
            last += offset + item.len();
        } else {
            order_ok = false;
            break;
        }
    }
    add_check(
        &mut checks,
        "required_order",
        order_ok,
        format!("固定順序: {}", contract.required_order.join(" -> ")),
    );

    for term in &contract.terms {
        let passed = view.prose_paragraphs.iter().any(|context| {
            if !context.contains(&term.term) {
                return false;
            }
            match term.status.as_str() {
                "coined" => has_coined_term_provenance(context),
                "established" | "source-specific" => term
                    .source
                    .as_deref()
                    .is_some_and(|name| context.contains(name)),
                "ordinary" => true,
                _ => false,
            }
        });
        add_check(
            &mut checks,
            "term_provenance",
            passed,
            format!("{} ({})", term.term, term.status),
        );
    }

    let registered = contract
        .terms
        .iter()
        .map(|item| item.term.as_str())
        .chain(contract.proper_nouns.iter().map(String::as_str))
        .chain(contract.accepted_labels.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    let quoted = Regex::new(r"[「《]([^」》]{2,24})[」》]").expect("valid label regex");
    let unregistered = quoted
        .captures_iter(main)
        .map(|capture| capture[1].to_owned())
        .filter(|term| !registered.contains(term.as_str()))
        .collect::<BTreeSet<_>>();
    if !unregistered.is_empty() {
        add_review(
            &mut checks,
            "unregistered_label",
            format!(
                "契約にないラベル候補: {}",
                unregistered.into_iter().collect::<Vec<_>>().join(", ")
            ),
        );
    }

    let caveat = Regex::new(
        r"必ずしも.{0,30}ない|可能性がある|対象に含めない|直接示すものではない|わけではない",
    )
    .expect("valid caveat regex");
    let caveats = caveat
        .find_iter(main)
        .map(|item| item.as_str())
        .collect::<Vec<_>>();
    if contract.reject_defensive_caveats {
        add_check(
            &mut checks,
            "defensive_caveats",
            caveats.is_empty(),
            if caveats.is_empty() {
                "防御的留保テンプレートなし".to_owned()
            } else {
                format!("候補: {}", caveats.join(", "))
            },
        );
    } else if !caveats.is_empty() {
        add_review(
            &mut checks,
            "defensive_caveats",
            format!("候補: {}", caveats.join(", ")),
        );
    }

    let outline = academic_outline(&view);
    let first_sentences = outline
        .iter()
        .filter(|entry| matches!(entry.kind.as_str(), "heading" | "lead"))
        .cloned()
        .collect::<Vec<_>>();
    let section_count = outline
        .iter()
        .filter(|entry| entry.kind == "heading")
        .count();
    add_check(
        &mut checks,
        "section_bridge_contract",
        section_count < 2 || !contract.section_bridges.is_empty(),
        if section_count < 2 {
            "節が一つのため節間接続の契約は不要です".to_owned()
        } else if contract.section_bridges.is_empty() {
            "複数節の原稿にはsection_bridgesを記録します".to_owned()
        } else {
            "節間接続の契約を確認".to_owned()
        },
    );
    audit_bridges(&mut checks, &outline, &contract.section_bridges);
    audit_first_sentence_order(&mut checks, &outline, &contract.first_sentence_order);
    audit_citations(&mut checks, &view, &contract.citation_aliases);
    audit_proper_nouns(&mut checks, main, &contract.proper_nouns);
    audit_notes(&mut checks, &view, &contract.note_decisions);

    let profile = &contract.style_profile;
    let style = style_observations(&view, profile);
    add_check(
        &mut checks,
        "style_sentence_length",
        style.sentence_length_mean >= profile.sentence_length_mean_min
            && style.sentence_length_mean <= profile.sentence_length_mean_max,
        format!("平均文長: {}", style.sentence_length_mean),
    );
    add_check(
        &mut checks,
        "style_citation_leads",
        style.citation_led_paragraph_ratio <= profile.citation_led_paragraph_ratio_max,
        format!(
            "著者名・年で始まる段落率: {}",
            style.citation_led_paragraph_ratio
        ),
    );
    if profile.discovery_marker_at_paragraph_end {
        let misplaced = view
            .prose_paragraphs
            .iter()
            .filter(|paragraph| {
                paragraph.contains("とわかる") && !paragraph.trim_end().ends_with("とわかる。")
            })
            .count();
        add_check(
            &mut checks,
            "style_discovery_marker",
            misplaced == 0,
            format!("「とわかる」が段落末以外にある段落: {misplaced}"),
        );
    }

    let delivery_paths_complete = matches!(
        (paths.docx, paths.pdf, paths.template, paths.export_record),
        (Some(_), Some(_), Some(_), Some(_))
    );
    let any_delivery_path = [paths.docx, paths.pdf, paths.template, paths.export_record]
        .iter()
        .any(Option::is_some);
    if any_delivery_path && !delivery_paths_complete {
        add_check(
            &mut checks,
            "delivery_artifacts",
            false,
            "提出監査には --docx、--pdf、--template、--export-record をすべて指定します",
        );
    } else if !delivery_paths_complete {
        add_review(
            &mut checks,
            "delivery_readiness",
            "原稿監査のみ完了。提出成果物は対象形式に対応する検証手順で別途確認します",
        );
    }
    audit_sync(&mut checks, &view, contract, paths)?;
    if let (Some(template), Some(docx)) = (paths.template, paths.docx) {
        audit_ooxml(&mut checks, template, docx, &contract.docx_mutable_entries)?;
    }
    if let Some(docx) = paths.docx {
        audit_docx_layout(&mut checks, docx, &contract.docx_layout_order)?;
    }
    if let Some(record) = paths.export_record {
        audit_export_record(&mut checks, record, paths)?;
    }
    let passed = checks.iter().all(|item| item.status != "fail");
    let delivery_ready = passed
        && delivery_paths_complete
        && checks
            .iter()
            .any(|item| item.id == "pdf_visual_review_declaration" && item.status == "pass");
    Ok(AcademicReport {
        passed,
        delivery_ready,
        checks,
        first_sentences,
        style,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs::File;
    use std::io::Write;

    use tempfile::tempdir;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::{
        AcademicContract, ArtifactPaths, academic_outline, audit, audit_citations,
        audit_docx_layout, audit_notes, audit_ooxml, audit_sync, explicit_citation_ids,
        file_sha256, has_coined_term_provenance, source_view, style_observations,
    };

    fn write_docx(path: &std::path::Path, text: &str, style: &str, extra: Option<(&str, &str)>) {
        write_docx_with_layout(path, text, style, "", extra);
    }

    fn write_docx_with_layout(
        path: &std::path::Path,
        text: &str,
        style: &str,
        layout: &str,
        extra: Option<(&str, &str)>,
    ) {
        write_docx_with_entries(
            path,
            text,
            style,
            layout,
            &extra.into_iter().collect::<Vec<_>>(),
        );
    }

    fn write_docx_with_entries(
        path: &std::path::Path,
        text: &str,
        style: &str,
        layout: &str,
        extras: &[(&str, &str)],
    ) {
        let file = File::create(path).expect("create docx");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        let content_types = extras
            .iter()
            .find_map(|(name, contents)| (*name == "[Content_Types].xml").then_some(*contents))
            .unwrap_or("<Types/>");
        zip.start_file("[Content_Types].xml", options)
            .expect("content types");
        zip.write_all(content_types.as_bytes())
            .expect("write content types");
        zip.start_file("word/styles.xml", options).expect("styles");
        zip.write_all(style.as_bytes()).expect("write styles");
        zip.start_file("word/document.xml", options)
            .expect("document");
        zip.write_all(
            format!(
                r#"<w:document xmlns:w="urn:test"><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p>{layout}</w:body></w:document>"#
            )
            .as_bytes(),
        )
        .expect("write document");
        for (name, contents) in extras {
            if *name == "[Content_Types].xml" {
                continue;
            }
            zip.start_file(name, options).expect("extra entry");
            zip.write_all(contents.as_bytes())
                .expect("write extra entry");
        }
        zip.finish().expect("finish docx");
    }

    fn write_pdf(path: &std::path::Path, text: &str) {
        let text = text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let content = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        std::fs::write(path, bytes).expect("write PDF");
    }

    fn write_export_record(
        path: &std::path::Path,
        source: &std::path::Path,
        docx: &std::path::Path,
        pdf: &std::path::Path,
        pdf_visual_reviewed: bool,
    ) {
        let record = format!(
            r#"{{"exporter":"Microsoft Word","pdf_visual_reviewed":{pdf_visual_reviewed},"source_sha256":"{}","docx_sha256":"{}","pdf_sha256":"{}"}}"#,
            file_sha256(source).expect("source hash"),
            file_sha256(docx).expect("docx hash"),
            file_sha256(pdf).expect("pdf hash"),
        );
        std::fs::write(path, record).expect("write export record");
    }

    fn contract() -> AcademicContract {
        serde_json::from_str(
            r#"{
                "version": 1,
                "central_claim": "Policy body has evidence.",
                "explanandum": "Policy",
                "allow_formal_research_question": false,
                "reject_defensive_caveats": true,
                "required_order": ["Policy"],
                "terms": [{"term": "Policy", "status": "ordinary"}],
                "proper_nouns": [],
                "first_sentence_order": ["Policy body has evidence."],
                "section_bridges": [],
                "note_decisions": [],
                "style_profile": {
                    "sentence_length_mean_min": 1.0,
                    "sentence_length_mean_max": 100.0,
                    "citation_led_paragraph_ratio_max": 1.0
                }
            }"#,
        )
        .expect("contract")
    }

    #[test]
    fn artifact_audit_checks_sync_and_template_invariants() {
        let dir = tempdir().expect("tempdir");
        let source = dir.path().join("paper.md");
        let template = dir.path().join("template.docx");
        let docx = dir.path().join("paper.docx");
        let pdf = dir.path().join("paper.pdf");
        let record = dir.path().join("delivery-record.json");
        std::fs::write(&source, "# Introduction\n\nPolicy body has evidence.\n").expect("source");
        write_docx(&template, "雛形", "<styles>fixed</styles>", None);
        write_docx(
            &docx,
            "Introduction Policy body has evidence.",
            "<styles>fixed</styles>",
            None,
        );
        write_pdf(&pdf, "Introduction Policy body has evidence.");
        write_export_record(&record, &source, &docx, &pdf, true);

        let report = audit(
            &std::fs::read_to_string(&source).expect("read source"),
            &contract(),
            &ArtifactPaths {
                source: &source,
                docx: Some(&docx),
                pdf: Some(&pdf),
                template: Some(&template),
                export_record: Some(&record),
            },
        )
        .expect("audit");
        assert!(report.passed, "{report:#?}");
        assert!(report.delivery_ready);

        write_export_record(&record, &source, &docx, &pdf, false);
        let unreviewed = audit(
            &std::fs::read_to_string(&source).expect("read source"),
            &contract(),
            &ArtifactPaths {
                source: &source,
                docx: Some(&docx),
                pdf: Some(&pdf),
                template: Some(&template),
                export_record: Some(&record),
            },
        )
        .expect("audit");
        assert!(!unreviewed.delivery_ready);
        assert!(
            unreviewed
                .checks
                .iter()
                .any(|item| item.id == "pdf_visual_review_declaration" && item.status == "fail")
        );

        let partial = audit(
            &std::fs::read_to_string(&source).expect("read source"),
            &contract(),
            &ArtifactPaths {
                source: &source,
                docx: Some(&docx),
                pdf: None,
                template: None,
                export_record: None,
            },
        )
        .expect("audit");
        assert!(!partial.passed);
        assert!(
            partial
                .checks
                .iter()
                .any(|item| item.id == "delivery_artifacts" && item.status == "fail")
        );

        write_docx(
            &docx,
            "Introduction Policy body has evidence.",
            "<styles>changed</styles>",
            None,
        );
        let changed = audit(
            &std::fs::read_to_string(&source).expect("read source"),
            &contract(),
            &ArtifactPaths {
                source: &source,
                docx: Some(&docx),
                pdf: Some(&pdf),
                template: Some(&template),
                export_record: None,
            },
        )
        .expect("audit");
        assert!(!changed.passed);
        assert!(
            changed
                .checks
                .iter()
                .any(|item| item.id == "ooxml_invariants" && item.status == "fail")
        );

        write_docx(
            &docx,
            "Introduction Policy body has evidence.",
            "<styles>fixed</styles>",
            Some(("word/header1.xml", "<header>new</header>")),
        );
        let added = audit(
            &std::fs::read_to_string(&source).expect("read source"),
            &contract(),
            &ArtifactPaths {
                source: &source,
                docx: Some(&docx),
                pdf: Some(&pdf),
                template: Some(&template),
                export_record: None,
            },
        )
        .expect("audit");
        assert!(
            added
                .checks
                .iter()
                .any(|item| item.id == "ooxml_invariants" && item.status == "fail")
        );
    }

    #[test]
    fn document_layout_changes_fail_while_text_replacement_passes() {
        let dir = tempdir().expect("tempdir");
        let source = dir.path().join("paper.md");
        let template = dir.path().join("template.docx");
        let docx = dir.path().join("paper.docx");
        let layout =
            r#"<w:sectPr><w:pgMar w:left="100"/><w:cols w:num="2"/><w:pgBorders/></w:sectPr>"#;
        std::fs::write(&source, "# Introduction\n\nPolicy body has evidence.\n").expect("source");
        write_docx_with_layout(
            &template,
            "placeholder",
            "<styles>fixed</styles>",
            layout,
            None,
        );
        write_docx_with_layout(
            &docx,
            "Introduction Policy body has evidence.",
            "<styles>fixed</styles>",
            layout,
            None,
        );
        let unchanged = audit(
            &std::fs::read_to_string(&source).expect("read source"),
            &contract(),
            &ArtifactPaths {
                source: &source,
                docx: Some(&docx),
                pdf: None,
                template: Some(&template),
                export_record: None,
            },
        )
        .expect("audit");
        assert!(
            unchanged
                .checks
                .iter()
                .any(|item| item.id == "ooxml_invariants" && item.status == "pass")
        );

        write_docx_with_layout(
            &docx,
            "Introduction Policy body has evidence.",
            "<styles>fixed</styles>",
            r#"<w:sectPr><w:pgMar w:left="200"/><w:cols w:num="2"/><w:pgBorders/></w:sectPr>"#,
            None,
        );
        let changed = audit(
            &std::fs::read_to_string(&source).expect("read source"),
            &contract(),
            &ArtifactPaths {
                source: &source,
                docx: Some(&docx),
                pdf: None,
                template: Some(&template),
                export_record: None,
            },
        )
        .expect("audit");
        assert!(
            changed
                .checks
                .iter()
                .any(|item| item.id == "ooxml_invariants" && item.status == "fail")
        );
    }

    #[test]
    fn sync_is_bidirectional_and_normalizes_links_tables_notes_and_references() {
        let dir = tempdir().expect("tempdir");
        let source_path = dir.path().join("paper.md");
        let docx = dir.path().join("paper.docx");
        let pdf = dir.path().join("paper.pdf");
        let source = "# Paper\n\nBody [link](https://example.com).[^1]\n\n| A | B |\n| - | - |\n| one | two |\n\n[^1]: Note text\n  continues here.\n\n# References\n\n1) Sano 2024a.\n";
        std::fs::write(&source_path, source).expect("source");
        let view = source_view(source, &[]);
        write_docx(&docx, &view.sync_text, "<styles>fixed</styles>", None);
        write_pdf(&pdf, &view.sync_text);
        let extracted = pdf_extract::extract_text(&pdf).expect("extract PDF");
        assert_eq!(
            super::normalized(&view.sync_text),
            super::normalized(&extracted),
            "PDF text extraction must preserve the fixture"
        );
        let mut checks = Vec::new();
        audit_sync(
            &mut checks,
            &view,
            &contract(),
            &ArtifactPaths {
                source: &source_path,
                docx: Some(&docx),
                pdf: Some(&pdf),
                template: None,
                export_record: None,
            },
        )
        .expect("sync");
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );

        write_docx(
            &docx,
            &format!("{} Old duplicate paragraph.", view.sync_text),
            "<styles>fixed</styles>",
            None,
        );
        write_pdf(&pdf, &format!("{} Old PDF paragraph.", view.sync_text));
        let mut mismatched = Vec::new();
        audit_sync(
            &mut mismatched,
            &view,
            &contract(),
            &ArtifactPaths {
                source: &source_path,
                docx: Some(&docx),
                pdf: Some(&pdf),
                template: None,
                export_record: None,
            },
        )
        .expect("sync");
        assert!(mismatched.iter().all(|item| item.status == "fail"));
    }

    #[test]
    fn citations_are_heading_scoped_bidirectional_and_detect_duplicates() {
        let good = source_view(
            "本文では参考文献を整理する。Sano & Tanaka (2024a) と 佐野・田中（2024b）を使う。\n\n# References\n\nSano, Tanaka (2024a).\n佐野・田中（2024b）.\n",
            &[],
        );
        let mut checks = Vec::new();
        audit_citations(&mut checks, &good, &std::collections::BTreeMap::new());
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );

        let bad = source_view(
            "Sano (2024a) を使う。\n\n# References\n\n1) Sano (2024b).\n2) Other (2024a).\n3) Other (2024a).\n",
            &[],
        );
        let mut checks = Vec::new();
        audit_citations(&mut checks, &bad, &std::collections::BTreeMap::new());
        assert!(
            checks
                .iter()
                .any(|item| item.id == "citation_reference_match" && item.status == "fail")
        );
        assert!(
            checks
                .iter()
                .any(|item| item.id == "uncited_references" && item.status == "fail")
        );
        assert!(
            checks
                .iter()
                .any(|item| item.id == "duplicate_references" && item.status == "fail")
        );
    }

    #[test]
    fn citations_accept_decorated_headings_list_markers_surnames_and_single_publication_year() {
        let good = source_view(
            "本文中の一般語として参考文献を説明する。秋吉（2000）、Sano (2024a)、田中（2024b）を使う。\n\n| 層 | 原典 |\n| - | - |\n| 戦略 | van de Velde（1999） |\n\n## ―――参考文献―――\n\n- 秋吉貴雄（2000）『行政計画』。doi:10.1000/2010.2025\n1) Sano (2024a). https://example.test/2011\n１）田中太郎（2024b）.\n[1] van de Velde (1999).\n[2] Other (1998).\n",
            &[],
        );
        let mut checks = Vec::new();
        audit_citations(&mut checks, &good, &std::collections::BTreeMap::new());
        assert!(
            checks
                .iter()
                .find(|item| item.id == "citation_reference_match")
                .is_some_and(|item| item.status == "pass"),
            "{checks:#?}"
        );
        assert!(
            checks
                .iter()
                .find(|item| item.id == "uncited_references")
                .is_some_and(|item| item.status == "fail"),
            "the uncited [1] entry must remain a real bibliography entry: {checks:#?}"
        );
        assert!(
            !good.references[0].starts_with('-')
                && !good.references[1].starts_with('1')
                && !good.references[2].starts_with('１'),
            "list markers must not become author text: {:?}",
            good.references
        );
    }

    #[test]
    fn citation_keys_keep_japanese_authors_after_prose_and_in_tables() {
        let keys = super::citation_keys(
            "政策規範である。佐野亘ほか（2021）は基準を示す。\n| 出典 | 内閣官房地域未来戦略本部事務局・内閣府地方創生推進室（2025、2026） |",
        );
        assert!(keys.contains("佐野亘:2021"), "{keys:?}");
        assert!(
            keys.contains("内閣官房地域未来戦略本部事務局:2025"),
            "{keys:?}"
        );
    }

    #[test]
    fn parenthetical_citations_match_references_and_detect_missing_entries() {
        for citation in [
            "先行研究（佐野, 2021）を参照した。",
            "先行研究(佐野, 2021)を参照した。",
        ] {
            let source = format!("{citation}\n\n# 参考文献\n\n佐野（2021）『研究』。\n");
            let mut checks = Vec::new();
            audit_citations(&mut checks, &source_view(&source, &[]), &BTreeMap::new());
            assert!(
                checks.iter().all(|item| item.status == "pass"),
                "{checks:#?}"
            );

            let mut missing = Vec::new();
            audit_citations(&mut missing, &source_view(citation, &[]), &BTreeMap::new());
            assert!(
                missing
                    .iter()
                    .any(|item| item.id == "citation_reference_match" && item.status == "fail"),
                "a missing reference must be detected: {missing:#?}"
            );
        }

        let source = "Evidence (Smith, 2021; Jones, 2022).\n\n# References\n\nSmith (2021).\nJones (2022).\n";
        let mut checks = Vec::new();
        audit_citations(&mut checks, &source_view(source, &[]), &BTreeMap::new());
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );
    }

    #[test]
    fn word_xml_text_preserves_ampersands() {
        let text = super::word_xml_text(
            br#"<w:document xmlns:w="urn:test"><w:body><w:p><w:r><w:t>Operations &amp; Production</w:t></w:r></w:p></w:body></w:document>"#,
        )
        .expect("Word XML text");
        assert_eq!(text.trim(), "Operations & Production");
    }

    #[test]
    fn ooxml_allows_article_content_additions_but_preserves_template_assets_and_sections() {
        let dir = tempdir().expect("tempdir");
        let template = dir.path().join("template.docx");
        let docx = dir.path().join("submission.docx");
        let layout = r#"<w:sectPr><w:pgSz w:w="11906"/><w:pgMar w:left="100"/><w:cols w:num="2"/><w:pgBorders><w:top w:val="single"/></w:pgBorders><w:headerReference w:id="rId1"/></w:sectPr>"#;
        let template_entries = [
            (
                "[Content_Types].xml",
                r#"<Types><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/main"/></Types>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Type="header" Target="header1.xml"/></Relationships>"#,
            ),
            ("word/header1.xml", "<header>fixed</header>"),
            ("word/media/template.png", "template image"),
        ];
        write_docx_with_entries(
            &template,
            "placeholder",
            "<styles>fixed</styles>",
            layout,
            &template_entries,
        );
        let submission_entries = [
            (
                "[Content_Types].xml",
                r#"<Types><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/main"/><Override PartName="/word/media/article.png" ContentType="image/png"/></Types>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Type="header" Target="header1.xml"/><Relationship Id="rId2" Type="image" Target="media/article.png"/></Relationships>"#,
            ),
            ("word/header1.xml", "<header>fixed</header>"),
            ("word/media/template.png", "template image"),
            ("word/media/article.png", "article image"),
        ];
        write_docx_with_entries(
            &docx,
            "Article body",
            "<styles>fixed</styles>",
            &format!(
                "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>table</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:drawing/>{layout}"
            ),
            &submission_entries,
        );
        let mut checks = Vec::new();
        audit_ooxml(
            &mut checks,
            &template,
            &docx,
            &contract().docx_mutable_entries,
        )
        .expect("OOXML audit");
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );

        write_docx_with_entries(
            &docx,
            "Article body",
            "<styles>fixed</styles>",
            &layout.replace("w:left=\"100\"", "w:left=\"200\""),
            &submission_entries,
        );
        let mut section_changed = Vec::new();
        audit_ooxml(
            &mut section_changed,
            &template,
            &docx,
            &contract().docx_mutable_entries,
        )
        .expect("OOXML audit");
        assert!(section_changed.iter().any(|item| item.status == "fail"));

        let changed_header = [
            ("[Content_Types].xml", submission_entries[0].1),
            ("word/_rels/document.xml.rels", submission_entries[1].1),
            ("word/header1.xml", "<header>changed</header>"),
            ("word/media/template.png", "template image"),
            ("word/media/article.png", "article image"),
        ];
        write_docx_with_entries(
            &docx,
            "Article body",
            "<styles>fixed</styles>",
            layout,
            &changed_header,
        );
        let mut header_changed = Vec::new();
        audit_ooxml(
            &mut header_changed,
            &template,
            &docx,
            &contract().docx_mutable_entries,
        )
        .expect("OOXML audit");
        assert!(header_changed.iter().any(|item| item.status == "fail"));
    }

    #[test]
    fn docx_footnotes_sync_without_order_dependence_and_reject_extra_duplicates() {
        let dir = tempdir().expect("tempdir");
        let source_path = dir.path().join("paper.md");
        let docx = dir.path().join("paper.docx");
        let source = "Body.[^a] Also body.[^b]\n\n[^a]: Note A.\n[^b]: Note B.\n";
        std::fs::write(&source_path, source).expect("source");
        let notes = r#"<w:footnotes xmlns:w="urn:test"><w:footnote w:id="1"><w:p><w:r><w:t>Note B.</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Note A.</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
        write_docx_with_entries(
            &docx,
            "Body. Also body.",
            "<styles>fixed</styles>",
            "",
            &[("word/footnotes.xml", notes)],
        );
        let view = source_view(source, &[]);
        let mut checks = Vec::new();
        audit_sync(
            &mut checks,
            &view,
            &contract(),
            &ArtifactPaths {
                source: &source_path,
                docx: Some(&docx),
                pdf: None,
                template: None,
                export_record: None,
            },
        )
        .expect("sync");
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );

        let duplicate_notes = notes.replace("</w:footnotes>", "<w:footnote w:id=\"3\"><w:p><w:r><w:t>Note A.</w:t></w:r></w:p></w:footnote></w:footnotes>");
        write_docx_with_entries(
            &docx,
            "Body. Also body.",
            "<styles>fixed</styles>",
            "",
            &[("word/footnotes.xml", &duplicate_notes)],
        );
        let mut duplicated = Vec::new();
        audit_sync(
            &mut duplicated,
            &view,
            &contract(),
            &ArtifactPaths {
                source: &source_path,
                docx: Some(&docx),
                pdf: None,
                template: None,
                export_record: None,
            },
        )
        .expect("sync");
        assert!(duplicated.iter().all(|item| item.status == "fail"));
    }

    #[test]
    fn body_checks_notes_and_style_exclude_markdown_outside_the_body() {
        let source = "---\nclaim: Policy body has evidence.\n---\n<!-- Policy -->\n```text\nPolicy\n```\n\n本文。[^missing]\n\n[^1]: Note text\n  continues here.\n\n# References\n\n1) Long Reference Sentence That Must Not Change Style Measurements 2024.\n";
        let view = source_view(source, &[]);
        assert!(!view.main_text.contains("Policy"));
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("paper.md");
        std::fs::write(&path, source).expect("source");
        let report = audit(
            source,
            &contract(),
            &ArtifactPaths {
                source: &path,
                docx: None,
                pdf: None,
                template: None,
                export_record: None,
            },
        )
        .expect("audit");
        for id in [
            "central_claim",
            "explanandum",
            "required_order",
            "term_provenance",
        ] {
            assert!(
                report
                    .checks
                    .iter()
                    .any(|item| item.id == id && item.status == "fail"),
                "{id} must not pass from front matter, comments, code, notes, or references"
            );
        }
        let style = style_observations(&view, &contract().style_profile);
        let baseline_style =
            style_observations(&source_view("本文。", &[]), &contract().style_profile);
        assert_eq!(style.paragraph_count, 1);
        assert_eq!(style.sentence_length_mean, 3.0);
        assert_eq!(
            style.sentence_length_mean,
            baseline_style.sentence_length_mean
        );
        let mut note_checks = Vec::new();
        audit_notes(
            &mut note_checks,
            &view,
            &[super::NoteDecision {
                marker: "[^1]".to_owned(),
                disposition: "note".to_owned(),
                text: "Note text continues here.".to_owned(),
            }],
        );
        assert!(
            note_checks
                .iter()
                .any(|item| item.id == "note_disposition" && item.status == "fail")
        );
        assert!(
            note_checks
                .iter()
                .any(|item| item.id == "note_references" && item.status == "fail")
        );

        let valid = source_view(
            "本文の補足[^1]。\n\n[^1]: Note text\n  continues here.\n",
            &[],
        );
        let decision = super::NoteDecision {
            marker: "[^1]".to_owned(),
            disposition: "note".to_owned(),
            text: "Note text continues here.".to_owned(),
        };
        let mut valid_checks = Vec::new();
        audit_notes(&mut valid_checks, &valid, std::slice::from_ref(&decision));
        assert!(valid_checks.iter().all(|item| item.status == "pass"));

        let duplicate = source_view(
            "Note text continues here.[^1]\n\n[^1]: Note text\n  continues here.\n",
            &[],
        );
        let mut duplicate_checks = Vec::new();
        audit_notes(&mut duplicate_checks, &duplicate, &[decision]);
        assert!(
            duplicate_checks
                .iter()
                .any(|item| item.id == "note_disposition" && item.status == "fail")
        );
    }

    #[test]
    fn japanese_note_sections_are_matched_to_inline_references() {
        let valid = source_view(
            "本文の補足を注1）に示す。\n\n## ―――注―――\n\n注1）日本語注記の本文である。\n\n## ―――参考文献―――\n",
            &[],
        );
        assert!(!valid.main_text.contains("日本語注記の本文"));
        let decision = super::NoteDecision {
            marker: "注１）".to_owned(),
            disposition: "note".to_owned(),
            text: "日本語注記の本文である。".to_owned(),
        };
        let mut checks = Vec::new();
        audit_notes(&mut checks, &valid, std::slice::from_ref(&decision));
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );

        let unused = source_view(
            "本文。\n\n## ―――注―――\n\n注1）日本語注記の本文である。\n",
            &[],
        );
        let mut unused_checks = Vec::new();
        audit_notes(&mut unused_checks, &unused, &[decision]);
        assert!(
            unused_checks
                .iter()
                .any(|item| item.id == "note_references" && item.status == "fail")
        );
    }

    #[test]
    fn ooxml_rejects_first_page_rule_changes() {
        let dir = tempdir().expect("tempdir");
        let template = dir.path().join("template.docx");
        let docx = dir.path().join("submission.docx");
        let layout = r#"<w:p><w:r><w:pict><v:line id="first-page-rule" style="width:10pt" from="0pt,1pt" to="100pt,1pt"/></w:pict></w:r></w:p><w:sectPr><w:type w:val="continuous"/><w:pgSz w:w="11906"/></w:sectPr>"#;
        write_docx_with_layout(
            &template,
            "placeholder",
            "<styles>fixed</styles>",
            layout,
            None,
        );
        write_docx_with_layout(&docx, "article", "<styles>fixed</styles>", layout, None);
        let mut unchanged = Vec::new();
        audit_ooxml(
            &mut unchanged,
            &template,
            &docx,
            &contract().docx_mutable_entries,
        )
        .expect("OOXML audit");
        assert!(unchanged.iter().all(|item| item.status == "pass"));

        write_docx_with_layout(
            &docx,
            "article",
            "<styles>fixed</styles>",
            &layout.replace("width:10pt", "width:20pt"),
            None,
        );
        assert!(
            !super::preserves_horizontal_vml_lines(
                layout.as_bytes(),
                layout.replace("width:10pt", "width:20pt").as_bytes(),
            )
            .expect("rule comparison")
        );
        let mut changed = Vec::new();
        audit_ooxml(
            &mut changed,
            &template,
            &docx,
            &contract().docx_mutable_entries,
        )
        .expect("OOXML audit");
        assert!(changed.iter().any(|item| item.status == "fail"));
    }

    #[test]
    fn docx_layout_checks_caption_pairing_and_contract_order() {
        let dir = tempdir().expect("tempdir");
        let docx = dir.path().join("paper.docx");
        let layout = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>表-1　比較</w:t></w:r></w:p><w:p><w:r><w:drawing/></w:r></w:p><w:p><w:r><w:t>図-1　流れ</w:t></w:r></w:p><w:p><w:r><w:t>結論</w:t></w:r></w:p>"#;
        write_docx_with_layout(
            &docx,
            "執政の創造性",
            "<styles>fixed</styles>",
            layout,
            None,
        );
        let mut checks = Vec::new();
        audit_docx_layout(
            &mut checks,
            &docx,
            &[
                "執政の創造性".to_owned(),
                "表-1".to_owned(),
                "図-1".to_owned(),
                "結論".to_owned(),
            ],
        )
        .expect("layout audit");
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );

        let broken = layout.replace(
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>表-1　比較</w:t></w:r></w:p>",
            "<w:p><w:r><w:t>表-1　比較</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        );
        write_docx_with_layout(
            &docx,
            "執政の創造性",
            "<styles>fixed</styles>",
            &broken,
            None,
        );
        let mut broken_checks = Vec::new();
        audit_docx_layout(&mut broken_checks, &docx, &[]).expect("layout audit");
        assert!(
            broken_checks
                .iter()
                .any(|item| item.id == "docx_table_figure_placement" && item.status == "fail")
        );
    }

    #[test]
    fn coined_term_provenance_accepts_research_authorship_and_operational_definition() {
        assert!(has_coined_term_provenance(
            "算出した人口割合を本研究では公共交通人口カバー率と呼ぶ。"
        ));
        assert!(has_coined_term_provenance(
            "七分類は、本研究プロジェクトが分析用に定めた操作的分類である。"
        ));
        assert!(!has_coined_term_provenance(
            "公共交通人口カバー率を分析に用いる。"
        ));
    }

    #[test]
    fn academic_outline_ignores_html_only_wrapper_lines() {
        let view = source_view(
            "# 結果\n\n結果の本文。\n\n<div>\n\n</div>\n\n# 考察\n\n考察の本文。\n",
            &[],
        );
        let outline = academic_outline(&view);
        assert!(
            outline
                .iter()
                .all(|entry| !matches!(entry.text.as_str(), "<div>" | "</div>")),
            "{outline:#?}"
        );
    }

    #[test]
    fn citations_accept_explicit_markdown_ids() {
        let view = source_view(
            "本文で第一資料[@source-a]と第二資料[@source_b]を使う。\n\n# 参考文献\n\n- [@source-a] 第一資料。\n- [@source_b] 第二資料。\n",
            &[],
        );
        let mut checks = Vec::new();
        audit_citations(&mut checks, &view, &BTreeMap::new());
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );
        assert!(explicit_citation_ids("連絡先はwriter@example.jpである。").is_empty());
    }

    #[test]
    fn accepted_labels_do_not_become_unregistered_label_reviews() {
        let source = "# Introduction\n\nPolicy body has evidence. 「単独」\n";
        let mut contract = contract();
        contract.accepted_labels = vec!["単独".to_owned()];
        let path = std::path::Path::new("paper.md");
        let report = audit(
            source,
            &contract,
            &ArtifactPaths {
                source: path,
                docx: None,
                pdf: None,
                template: None,
                export_record: None,
            },
        )
        .expect("academic audit");
        assert!(
            report
                .checks
                .iter()
                .all(|item| item.id != "unregistered_label"),
            "{report:#?}"
        );
    }
}
