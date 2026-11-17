//! 改稿前後の文書を文単位で対応づけ、意味が変わった可能性のある箇所を候補として返す。
//! 数値・否定・文末の働き・条件・識別子・引用・主語の増減を、形態素と表層で照合する。
//! 意味が同じかどうかは判定しない。候補が出ない変更も読み直す対象である。

mod align;
mod arguments;
mod endings;
mod modality;
mod negation;
mod protected;
mod relations;
mod tone;
mod units;
mod values;

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest, Sha256};

use self::align::{align, coverage, pair_units, regions};
use self::arguments::{
    Argument, actor_narrowed_findings, case_arguments, content_word_findings,
    nominalization_added_findings, proper_noun_findings, recipient_added_findings,
    subject_added_findings,
};
use self::endings::ending_kind;
use self::modality::{certainty_findings, condition_findings, frequency_findings};
use self::negation::negation_findings;
use self::protected::protected_findings;
use self::relations::{causal_findings, choice_findings, contrast_findings, structure_findings};
use self::tone::{
    addressee_findings, feeling_findings, promise_findings, register_findings, style_findings,
    tone_findings,
};
use self::units::{Unit, UnitKind, units};
pub(crate) use self::values::numbers;
use self::values::{citation_findings, identifier_findings, number_findings, quotation_findings};
use crate::Error;
use crate::morphology::Morphology;

/// 入力本文のSHA-256（16進小文字）。検査した本文と最終稿が同じかを照らすために返す。
pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffFinding {
    pub category: String,
    pub severity: String,
    /// 変更前の対象行。変更後だけにある箇所はNone。
    pub before_line: Option<usize>,
    /// 変更後の対象行。変更前だけにある箇所はNone。
    pub after_line: Option<usize>,
    pub before_excerpt: String,
    pub after_excerpt: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffStats {
    pub total_findings: usize,
    pub by_category: BTreeMap<String, usize>,
    /// 文書を文・箇条書き・見出し・引用行に分けた単位の数。
    pub before_units: usize,
    pub after_units: usize,
    /// 空白を除いて一致した単位の数。
    pub matched_units: usize,
    /// 位置だけが変わった単位の数（前後の合計）。
    pub moved_units: usize,
    /// 一致しない単位が連続する領域の数。
    pub changed_regions: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffReport {
    pub stats: DiffStats,
    pub findings: Vec<DiffFinding>,
}

const EXCERPT_CHARS: usize = 80;
const PAIR_SIMILARITY: f64 = 0.3;
const COVERAGE_KEPT: f64 = 0.5;

pub fn compare(before: &str, after: &str, morphology: &Morphology) -> Result<DiffReport, Error> {
    let before_units = units(before, morphology)?;
    let after_units = units(after, morphology)?;
    let pairs = align(&before_units, &after_units);
    let (regions, moved) = regions(&before_units, &after_units, &pairs);
    // 文書全体にある値は、別の文へ移しただけでも増減として数えない。
    let document_values = DocumentValues {
        numbers_before: numbers_in(&before_units),
        numbers_after: numbers_in(&after_units),
        text_before: before_units.iter().map(|unit| unit.text.as_str()).collect(),
    };

    let mut findings = Vec::new();
    for region in &regions {
        let before_block = region
            .before
            .iter()
            .map(|index| &before_units[*index])
            .collect::<Vec<_>>();
        let after_block = region
            .after
            .iter()
            .map(|index| &after_units[*index])
            .collect::<Vec<_>>();
        analyze_region(&before_block, &after_block, &document_values, &mut findings);
    }
    findings.extend(protected_findings(before, after));
    let document_register = register_findings(&before_units, &after_units);
    // 文書全体の文体を変えた場合は、文ごとの切り替えを重ねて示さない。
    if !document_register.is_empty() {
        findings.retain(|finding| finding.category != "sentence_register_changed");
    }
    findings.extend(document_register);
    findings.extend(tone_findings(&before_units, &after_units));
    findings.extend(addressee_findings(before, after));
    findings.sort_by_key(|finding| {
        (
            finding.before_line.or(finding.after_line),
            finding.after_line,
            finding.category.clone(),
        )
    });

    let mut by_category = BTreeMap::new();
    for finding in &findings {
        *by_category.entry(finding.category.clone()).or_default() += 1;
    }
    Ok(DiffReport {
        stats: DiffStats {
            total_findings: findings.len(),
            by_category,
            before_units: before_units.len(),
            after_units: after_units.len(),
            matched_units: pairs.len(),
            moved_units: moved,
            changed_regions: regions.len(),
        },
        findings,
    })
}

// ---- 領域ごとの照合 ----

fn excerpt(units: &[&Unit]) -> String {
    let joined = units
        .iter()
        .map(|unit| unit.raw_text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut excerpt = joined.chars().take(EXCERPT_CHARS).collect::<String>();
    if joined.chars().count() > EXCERPT_CHARS {
        excerpt.push('…');
    }
    excerpt
}

fn finding(
    category: &str,
    severity: &str,
    before: &[&Unit],
    after: &[&Unit],
    detail: String,
) -> DiffFinding {
    DiffFinding {
        category: category.to_owned(),
        severity: severity.to_owned(),
        before_line: before.first().map(|unit| unit.line),
        after_line: after.first().map(|unit| unit.line),
        before_excerpt: excerpt(before),
        after_excerpt: excerpt(after),
        detail,
    }
}

fn block_text(units: &[&Unit]) -> String {
    units
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn block_raw(units: &[&Unit]) -> String {
    units
        .iter()
        .map(|unit| unit.raw_text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 文書全体に現れる数値と、変更前の本文。
struct DocumentValues {
    numbers_before: BTreeSet<String>,
    numbers_after: BTreeSet<String>,
    text_before: String,
}

fn numbers_in(units: &[Unit]) -> BTreeSet<String> {
    units.iter().flat_map(|unit| numbers(&unit.text)).collect()
}

fn analyze_region(
    before: &[&Unit],
    after: &[&Unit],
    document_values: &DocumentValues,
    findings: &mut Vec<DiffFinding>,
) {
    if before.is_empty() {
        findings.push(finding(
            "sentence_added",
            "warn",
            before,
            after,
            format!(
                "変更前にない文が{}件あります。原文や資料に根拠がある内容か、新しい主体・原因・効果を書き足していないか確認してください。",
                after.len()
            ),
        ));
        return;
    }
    if after.is_empty() {
        findings.push(finding(
            "sentence_removed",
            "warn",
            before,
            after,
            format!(
                "変更後にない文が{}件あります。条件・例外・数値・理由が一緒に消えていないか確認してください。",
                before.len()
            ),
        ));
        return;
    }
    let claimed_by = pair_units(before, after);
    // 対応する文がなく、内容語も相手側に残っていない文は削除・追加として示し、
    // 残りの文（書き換えと、分割・結合で形を変えた文）だけを語句の照合にかける。
    let mut kept_before = Vec::new();
    for (b_index, b_unit) in before.iter().enumerate() {
        if claimed_by.contains(&Some(b_index)) || coverage(b_unit, after) >= COVERAGE_KEPT {
            kept_before.push(*b_unit);
        } else {
            findings.push(finding(
                "sentence_removed",
                "warn",
                &[b_unit],
                &[],
                "変更後に対応する文がありません。条件・例外・数値・理由が一緒に消えていないか確認してください。".to_owned(),
            ));
        }
    }
    let mut kept_after = Vec::new();
    for (a_index, a_unit) in after.iter().enumerate() {
        if claimed_by[a_index].is_some() || coverage(a_unit, before) >= COVERAGE_KEPT {
            kept_after.push(*a_unit);
        } else {
            findings.push(finding(
                "sentence_added",
                "warn",
                &[],
                &[a_unit],
                "変更前に対応する文がありません。原文や資料に根拠がある内容か、新しい主体・原因・効果を書き足していないか確認してください。".to_owned(),
            ));
        }
    }
    pair_findings(before, after, &claimed_by, findings);
    if kept_before.is_empty() || kept_after.is_empty() {
        return;
    }
    feeling_findings(&kept_before, &kept_after, findings);
    promise_findings(&kept_before, &kept_after, findings);
    number_findings(&kept_before, &kept_after, document_values, findings);
    certainty_findings(&kept_before, &kept_after, findings);
    frequency_findings(&kept_before, &kept_after, findings);
    condition_findings(&kept_before, &kept_after, findings);
    identifier_findings(&kept_before, &kept_after, findings);
    quotation_findings(&kept_before, &kept_after, findings);
    citation_findings(&kept_before, &kept_after, findings);
    subject_added_findings(&kept_before, &kept_after, findings);
    recipient_added_findings(&kept_before, &kept_after, findings);
    nominalization_added_findings(&kept_before, &kept_after, findings);
    proper_noun_findings(&kept_before, &kept_after, document_values, findings);
    content_word_findings(&kept_before, &kept_after, findings);
    actor_narrowed_findings(&kept_before, &kept_after, findings);
}

fn multiset_diff(before: &[String], after: &[String]) -> (Vec<String>, Vec<String>) {
    let mut counts = BTreeMap::<&str, i64>::new();
    for item in before {
        *counts.entry(item.as_str()).or_default() += 1;
    }
    for item in after {
        *counts.entry(item.as_str()).or_default() -= 1;
    }
    let mut lost = Vec::new();
    let mut added = Vec::new();
    for (item, count) in counts {
        for _ in 0..count.max(0) {
            lost.push(item.to_owned());
        }
        for _ in 0..(-count).max(0) {
            added.push(item.to_owned());
        }
    }
    (lost, added)
}

fn join_items(items: &[String]) -> String {
    const LIMIT: usize = 8;
    let mut text = items
        .iter()
        .take(LIMIT)
        .map(|item| format!("「{item}」"))
        .collect::<Vec<_>>()
        .join("、");
    if items.len() > LIMIT {
        text.push_str(&format!("ほか{}件", items.len() - LIMIT));
    }
    if text.is_empty() {
        text.push_str("なし");
    }
    text
}

/// 消えた項目か増えた項目があれば、その一覧を説明に入れた候補を1件作る。
fn item_change(
    category: &str,
    severity: &str,
    before: &[&Unit],
    after: &[&Unit],
    lost: &[String],
    added: &[String],
    describe: impl FnOnce(String, String) -> String,
) -> Option<DiffFinding> {
    (!lost.is_empty() || !added.is_empty()).then(|| {
        finding(
            category,
            severity,
            before,
            after,
            describe(join_items(lost), join_items(added)),
        )
    })
}

/// 同じ値の繰り返しは数えず、値の種類だけを比べる。
fn distinct(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// 対応する文の対について、文末の働きの変化と主語・目的語の入れ替わりを見る。
/// 対応のない文は、内容語が相手側に残っていなければ削除・追加として示す。
fn pair_findings(
    before: &[&Unit],
    after: &[&Unit],
    claimed_by: &[Option<usize>],
    findings: &mut Vec<DiffFinding>,
) {
    for (b_index, b_unit) in before.iter().enumerate() {
        let group = claimed_by
            .iter()
            .enumerate()
            .filter(|(_, claimed)| **claimed == Some(b_index))
            .map(|(a_index, _)| a_index)
            .collect::<Vec<_>>();
        let Some(&last_index) = group.last() else {
            continue;
        };
        let last_unit = after[last_index];
        let commas_before = b_unit.duplicate_commas;
        let commas_after = last_unit.duplicate_commas;
        if commas_after > commas_before && last_unit.kind != UnitKind::Quote {
            findings.push(finding(
                "duplicate_comma_added",
                "warn",
                &[b_unit],
                &[last_unit],
                "改稿後に連続する読点が増えました。修正時の重複であれば一つに戻してください。"
                    .into(),
            ));
        }
        negation_findings(b_unit, last_unit, findings);
        if group.len() == 1 {
            style_findings(b_unit, last_unit, findings);
        }
        structure_findings(b_unit, last_unit, findings);
        contrast_findings(b_unit, last_unit, findings);
        choice_findings(b_unit, last_unit, findings);
        causal_findings(b_unit, last_unit, findings);
        if b_unit.kind != UnitKind::Heading
            && last_unit.kind != UnitKind::Heading
            && (b_unit.ability_text != last_unit.ability_text
                || b_unit.end_mark != last_unit.end_mark)
        {
            let (before_kind, after_kind) = (ending_kind(b_unit), ending_kind(last_unit));
            // 体言止めを文にした変化は文体の調整として扱い、時制だけの変化と、受身・可能・尊敬の
            // どれにも読める「れる／られる」との行き来は info にする。
            let unmarked =
                |kind: &str| matches!(kind, "説明・断定" | "過去" | "体言止め" | "受身・可能");
            let severity = match (before_kind, after_kind) {
                (b, a) if b == a => None,
                (b, a) if b == "体言止め" || a == "体言止め" => {
                    (!unmarked(b) || !unmarked(a)).then_some("warn")
                }
                (b, a) if unmarked(b) && unmarked(a) => Some("info"),
                _ => Some("warn"),
            };
            if let Some(severity) = severity {
                findings.push(finding(
                    "sentence_function_changed",
                    severity,
                    &[b_unit],
                    &[last_unit],
                    format!(
                        "文末の働きが「{before_kind}」から「{after_kind}」に変わりました。依頼、許可、できること、自動で起きること、推量の区別を原文のとおりに保ってください。"
                    ),
                ));
            }
        }
        let first_unit = after[group[0]];
        let (before_subjects, before_objects) = case_arguments(&b_unit.tokens);
        let (after_subjects, after_objects) = case_arguments(&first_unit.tokens);
        let has = |arguments: &[Argument], form: &str| {
            arguments.iter().any(|argument| argument.form == form)
        };
        let swapped = before_subjects
            .iter()
            .filter(|subject| has(&after_objects, &subject.form))
            .find_map(|subject| {
                before_objects
                    .iter()
                    .find(|object| {
                        object.form != subject.form && has(&after_subjects, &object.form)
                    })
                    .map(|object| (subject.surface.clone(), object.surface.clone()))
            });
        if let Some((subject, object)) = swapped {
            findings.push(finding(
                "subject_object_swapped",
                "warn",
                &[b_unit],
                &[first_unit],
                format!(
                    "「{subject}」と「{object}」の主語・目的語が入れ替わっています。動作の向きが原文と同じか確認してください。"
                ),
            ));
        }
    }
}
