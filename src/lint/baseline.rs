//! 前回のlint JSONとの比較。findingの解消・新規・継続を分類する。
//! 対応づけは compare・check と同じ matching を使う。baseline JSON には本文がないため、
//! 抜粋の一致（全体→先頭）と行の近さだけで照合する。

use serde::Serialize;
use serde_json::Value;

use super::Finding;
use super::matching::{MatchKey, match_findings};

#[derive(Clone, Debug, Serialize)]
pub struct BaselineSummary {
    pub resolved: usize,
    pub new: usize,
    pub persisting: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct BaselineReport {
    pub file: String,
    pub file_status: String,
    pub summary: BaselineSummary,
    pub resolved: Vec<Value>,
}

fn summarize(
    findings: &[Finding],
    file: String,
    file_status: &str,
    resolved: Vec<Value>,
) -> BaselineReport {
    BaselineReport {
        file,
        file_status: file_status.to_owned(),
        summary: BaselineSummary {
            resolved: resolved.len(),
            new: findings
                .iter()
                .filter(|finding| finding.status.as_deref() == Some("new"))
                .count(),
            persisting: findings
                .iter()
                .filter(|finding| finding.status.as_deref() == Some("persisting"))
                .count(),
        },
        resolved,
    }
}

pub fn apply_baseline(
    findings: &mut [Finding],
    baseline: &Value,
    file: String,
) -> Result<BaselineReport, String> {
    let Some(object) = baseline.as_object() else {
        return Err("--baseline の内容が JSON オブジェクトではありません。baseline比較を無視して通常のlintを実行します。".to_owned());
    };
    let Some(items) = object.get("findings").and_then(Value::as_array) else {
        return Err("--baseline に 'findings' 配列が見つかりません。baseline比較を無視して通常のlintを実行します。".to_owned());
    };
    let valid = items
        .iter()
        .filter(|item| {
            item.get("category").and_then(Value::as_str).is_some()
                && item.get("excerpt").and_then(Value::as_str).is_some()
        })
        .collect::<Vec<_>>();
    let skipped = items.len() - valid.len();
    if skipped > 0 {
        eprintln!(
            "警告: --baseline の findings 配列内に不正な要素が{skipped}件あったため読み飛ばしました。"
        );
    }
    let before = valid
        .iter()
        .map(|item| MatchKey {
            line: item
                .get("line")
                .and_then(Value::as_u64)
                .and_then(|line| usize::try_from(line).ok())
                .unwrap_or(0),
            category: item["category"].as_str().unwrap_or_default(),
            excerpt: item["excerpt"].as_str().unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    let after = findings
        .iter()
        .map(|finding| MatchKey {
            line: finding.line,
            category: &finding.category,
            excerpt: &finding.excerpt,
        })
        .collect::<Vec<_>>();
    let matches = match_findings(&before, &after, None);
    let mut persisting = vec![false; findings.len()];
    for pair in &matches.persisting {
        persisting[pair.after] = true;
    }
    for (finding, kept) in findings.iter_mut().zip(persisting) {
        finding.status = Some(if kept { "persisting" } else { "new" }.to_owned());
    }
    let resolved = matches
        .resolved
        .iter()
        .map(|&index| valid[index].clone())
        .collect::<Vec<_>>();
    Ok(summarize(findings, file, "matched", resolved))
}

/// baselineに対応recordがないファイル用: 全findingを新規として集計する。
pub fn baseline_added(findings: &mut [Finding], file: String) -> BaselineReport {
    for finding in findings.iter_mut() {
        finding.status = Some("new".to_owned());
    }
    summarize(findings, file, "added", Vec::new())
}
