//! 文書ごとに「まず直す数件」を選ぶ。重大度と規則の確度で並べ、同じ規則に偏らないようにする。
//!
//! 確度は suiko-eval の品質表（eval/calibration.md「規則ごとの品質表と関門（2026-11-03）」と
//! 「評価専用の標本の追加（2026-11-04）」）による。
//! high: 人間文書でほぼ付かず、ラベル付き標本の条件を満たした規則と、表記・記法の機械的な誤り。
//! low: 人間文書の多くに付く規則（品質表のdemotion_candidate。人間文書に付く割合のWilson上限が
//! 0.15を超える）、評価中の規則、文書全体の統計（直す箇所を一つに指せない）。
//! medium: それ以外。標本が足りず判定できていない規則を含む。
//! 規則ごとの値は登録表（registry.rs）が持つ。

use serde::Serialize;

use super::{Finding, registry};

/// 同じ規則から選ぶ上限。一つの規則で一覧が埋まらないようにする。
const PER_CATEGORY: usize = 2;
/// 既定で示す件数。
pub const FIRST_FIX_LIMIT: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// 規則の確度。登録表（registry.rs）の値で、登録のないカテゴリはmediumとする。
pub fn confidence(category: &str) -> Confidence {
    registry::exact(category).map_or(Confidence::Medium, |rule| rule.confidence)
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 3,
        "warn" => 2,
        _ => 1,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct FirstFix {
    pub rank: usize,
    pub line: usize,
    pub category: String,
    pub severity: String,
    pub confidence: Confidence,
    pub excerpt: String,
}

/// 重大度→確度→行の順に並べ、規則ごとに2件までで上位`limit`件を返す。
pub fn first_fixes(findings: &[Finding], limit: usize) -> Vec<FirstFix> {
    let mut ordered = findings.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|finding| {
        (
            std::cmp::Reverse(severity_rank(&finding.severity)),
            std::cmp::Reverse(confidence(&finding.category)),
            finding.line,
        )
    });
    let mut per_category = std::collections::HashMap::<&str, usize>::new();
    let mut selected = Vec::new();
    for finding in ordered {
        if selected.len() == limit {
            break;
        }
        let count = per_category.entry(finding.category.as_str()).or_default();
        if *count == PER_CATEGORY {
            continue;
        }
        *count += 1;
        selected.push(FirstFix {
            rank: selected.len() + 1,
            line: finding.line,
            category: finding.category.clone(),
            severity: finding.severity.clone(),
            confidence: confidence(&finding.category),
            excerpt: finding.excerpt.clone(),
        });
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(line: usize, category: &str, severity: &str) -> Finding {
        Finding::new(line, category, "抜粋", severity, "")
    }

    #[test]
    fn first_fixes_prefer_severity_then_confidence_and_spread_categories() {
        let findings = [
            finding(1, "translationese", "info"),
            finding(2, "translationese", "info"),
            finding(3, "abstract_metaphor", "info"),
            finding(4, "calque_phrase", "info"),
            finding(5, "forbidden_phrase", "warn"),
            finding(6, "invisible_character", "warn"),
            finding(7, "invisible_character", "warn"),
            finding(8, "invisible_character", "warn"),
        ];
        let fixes = first_fixes(&findings, 5);
        let order = fixes
            .iter()
            .map(|fix| (fix.line, fix.category.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            [
                (6, "invisible_character"),
                (7, "invisible_character"),
                (5, "forbidden_phrase"),
                (3, "abstract_metaphor"),
                (4, "calque_phrase"),
            ]
        );
        assert_eq!(fixes[0].rank, 1);
        assert_eq!(confidence("vague_loanword"), Confidence::Low);
        assert_eq!(confidence("thin_evaluation"), Confidence::High);
        assert_eq!(confidence("calque_phrase"), Confidence::Medium);
        assert_eq!(confidence("sensory_role_mismatch"), Confidence::Medium);
        assert!(first_fixes(&[], 5).is_empty());
    }
}
