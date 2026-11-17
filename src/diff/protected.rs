//! 文書全体で比べる保護範囲: コードブロックと表。

use std::collections::HashMap;

use super::units::bigrams;
use super::values::{identifiers, numbers};
use super::{DiffFinding, EXCERPT_CHARS, PAIR_SIMILARITY, distinct, join_items, multiset_diff};
use crate::text::{fenced_lines, fenced_ranges};

fn fenced_blocks(raw: &str) -> Vec<LinedText> {
    // 改行コードだけの違い（LFとCRLF）は、コードの変更として数えない。
    let lines = raw
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    // 散文の検査と同じ規則で開きと閉じを決め、内側の短いフェンスでブロックを分けない。
    fenced_ranges(&lines)
        .into_iter()
        .map(|range| (range.start + 1, lines[range].join("\n")))
        .collect()
}

fn table_rows(raw: &str) -> Vec<LinedText> {
    let lines = raw.split('\n').collect::<Vec<_>>();
    let fenced = fenced_lines(&lines);
    lines
        .iter()
        .enumerate()
        .filter(|(index, _)| !fenced[*index])
        .map(|(index, line)| (index + 1, line.trim()))
        .filter(|(_, line)| {
            line.starts_with('|')
                && line.matches('|').count() >= 2
                && !line.chars().all(|ch| matches!(ch, '|' | '-' | ':' | ' '))
        })
        .map(|(line_no, line)| (line_no, line.to_owned()))
        .collect()
}

/// 行番号付きの本文（コードブロック1件、表の1行）。
type LinedText = (usize, String);

fn lined_diff(before: &[LinedText], after: &[LinedText]) -> (Vec<LinedText>, Vec<LinedText>) {
    let mut after_counts = HashMap::<&str, usize>::new();
    for (_, body) in after {
        *after_counts.entry(body.as_str()).or_default() += 1;
    }
    let mut before_counts = HashMap::<&str, usize>::new();
    for (_, body) in before {
        *before_counts.entry(body.as_str()).or_default() += 1;
    }
    let take = |counts: &mut HashMap<&str, usize>, body: &str| {
        counts
            .get_mut(body)
            .filter(|count| **count > 0)
            .map(|count| *count -= 1)
            .is_some()
    };
    let lost = before
        .iter()
        .filter(|(_, body)| !take(&mut after_counts, body))
        .cloned()
        .collect::<Vec<_>>();
    let added = after
        .iter()
        .filter(|(_, body)| !take(&mut before_counts, body))
        .cloned()
        .collect::<Vec<_>>();
    (lost, added)
}

pub(super) fn protected_findings(before: &str, after: &str) -> Vec<DiffFinding> {
    let mut findings = Vec::new();
    let (lost_blocks, added_blocks) = lined_diff(&fenced_blocks(before), &fenced_blocks(after));
    if !lost_blocks.is_empty() || !added_blocks.is_empty() {
        let excerpt_of = |blocks: &[(usize, String)]| {
            blocks
                .first()
                .map(|(_, body)| {
                    let mut text = body.chars().take(EXCERPT_CHARS).collect::<String>();
                    if body.chars().count() > EXCERPT_CHARS {
                        text.push('…');
                    }
                    text.replace('\n', "⏎")
                })
                .unwrap_or_default()
        };
        findings.push(DiffFinding {
            category: "protected_changed".to_owned(),
            severity: "warn".to_owned(),
            before_line: lost_blocks.first().map(|(line, _)| *line),
            after_line: added_blocks.first().map(|(line, _)| *line),
            before_excerpt: excerpt_of(&lost_blocks),
            after_excerpt: excerpt_of(&added_blocks),
            detail: format!(
                "コードブロックが変わりました（消えた{}件、増えた{}件）。推敲ではコード、コマンド、出力例を書き換えないでください。",
                lost_blocks.len(),
                added_blocks.len()
            ),
        });
    }
    findings.extend(table_findings(before, after));
    findings
}

fn text_similarity(left: &str, right: &str) -> f64 {
    let left_set = bigrams(left);
    let right_set = bigrams(right);
    let total = left_set.len() + right_set.len();
    if total == 0 {
        return 0.0;
    }
    2.0 * left_set.intersection(&right_set).count() as f64 / total as f64
}

/// 表の行を字面の近さで対応づけ、数値と識別子の変化を行ごとに示す。対応のない行は件数だけ示す。
fn table_findings(before: &str, after: &str) -> Vec<DiffFinding> {
    let (lost_rows, added_rows) = lined_diff(&table_rows(before), &table_rows(after));
    let mut findings = Vec::new();
    let mut used = vec![false; added_rows.len()];
    let mut unpaired_lost = Vec::new();
    for (before_line, before_row) in &lost_rows {
        let best = added_rows
            .iter()
            .enumerate()
            .filter(|(index, _)| !used[*index])
            .map(|(index, (_, row))| (index, text_similarity(before_row, row)))
            .filter(|(_, score)| *score >= PAIR_SIMILARITY)
            .max_by(|left, right| left.1.total_cmp(&right.1).then(right.0.cmp(&left.0)));
        let Some((index, _)) = best else {
            unpaired_lost.push((*before_line, before_row));
            continue;
        };
        used[index] = true;
        let (after_line, after_row) = &added_rows[index];
        let (lost_numbers, added_numbers) = multiset_diff(
            &distinct(numbers(before_row)),
            &distinct(numbers(after_row)),
        );
        let (lost_identifiers, added_identifiers) =
            multiset_diff(&identifiers(before_row), &identifiers(after_row));
        let value_changed = !(lost_numbers.is_empty()
            && added_numbers.is_empty()
            && lost_identifiers.is_empty()
            && added_identifiers.is_empty());
        let detail = if value_changed {
            format!(
                "表の行の数値や識別子が変わりました。消えた値: {} / 増えた値: {}。セルの値と単位が原文と同じか確認してください。",
                join_items(&[lost_numbers, lost_identifiers].concat()),
                join_items(&[added_numbers, added_identifiers].concat())
            )
        } else {
            "表の行の文言が変わりました。項目名や説明の意味が原文と同じか確認してください。"
                .to_owned()
        };
        findings.push(DiffFinding {
            category: "table_changed".to_owned(),
            severity: if value_changed { "warn" } else { "info" }.to_owned(),
            before_line: Some(*before_line),
            after_line: Some(*after_line),
            before_excerpt: before_row.clone(),
            after_excerpt: after_row.clone(),
            detail,
        });
    }
    let unpaired_added = used.iter().filter(|used| !**used).count();
    if !unpaired_lost.is_empty() || unpaired_added > 0 {
        // 対応がついた行ではなく、対応のない最初の行を示す。
        let first_lost = unpaired_lost.first();
        let first_added = added_rows
            .iter()
            .zip(&used)
            .find(|(_, used)| !**used)
            .map(|(row, _)| row);
        findings.push(DiffFinding {
            category: "table_changed".to_owned(),
            severity: "info".to_owned(),
            before_line: first_lost.map(|(line, _)| *line),
            after_line: first_added.map(|(line, _)| *line),
            before_excerpt: first_lost
                .map(|(_, row)| String::clone(row))
                .unwrap_or_default(),
            after_excerpt: first_added.map(|(_, row)| row.clone()).unwrap_or_default(),
            detail: format!(
                "表の行が増減しました（対応のない行: 消えた{}行、増えた{unpaired_added}行）。消えた行の値や条件が本文に移っているか確認してください。",
                unpaired_lost.len()
            ),
        });
    }
    findings
}
