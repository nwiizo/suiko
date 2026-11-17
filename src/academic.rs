//! 学術原稿の監査: 監査契約（用語・節のつなぎ・注・文体）と、原稿・DOCX・PDFの照合。
//! 原稿の読み取りは`source`、引用と注は`citations`、DOCXは`docx`、成果物との同期は`sync`。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::outline::{OutlineEntry, build_outline};

mod citations;
mod docx;
mod source;
mod sync;

use citations::{audit_citations, audit_notes};
use docx::{audit_docx_layout, audit_export_record, audit_ooxml};
use source::{SourceView, plain_markdown, source_view};
use sync::audit_sync;

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
mod tests;
