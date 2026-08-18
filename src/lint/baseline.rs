//! 前回のlint JSONとの比較。findingの解消・新規・継続を分類する。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::Finding;

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

fn baseline_key(category: &str, excerpt: &str) -> (String, String) {
    const CATEGORY_ONLY: &[&str] = &[
        "antithesis_repetition",
        "low_burstiness",
        "high_length_autocorrelation",
        "low_sentence_variance",
        "uniform_paragraph_structure",
        "low_lexical_diversity_ttr",
        "low_lexical_diversity_mtld",
    ];
    if CATEGORY_ONLY.contains(&category) {
        return (category.to_owned(), String::new());
    }
    let normalized = excerpt
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .take(20)
        .collect::<String>();
    (category.to_owned(), normalized)
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
    let mut buckets = BTreeMap::<(String, String), Vec<Value>>::new();
    let mut skipped = 0;
    for item in items {
        let Some(category) = item.get("category").and_then(Value::as_str) else {
            skipped += 1;
            continue;
        };
        let Some(excerpt) = item.get("excerpt").and_then(Value::as_str) else {
            skipped += 1;
            continue;
        };
        buckets
            .entry(baseline_key(category, excerpt))
            .or_default()
            .push(item.clone());
    }
    if skipped > 0 {
        eprintln!(
            "警告: --baseline の findings 配列内に不正な要素が{skipped}件あったため読み飛ばしました。"
        );
    }
    for finding in findings.iter_mut() {
        let key = baseline_key(&finding.category, &finding.excerpt);
        if let Some(bucket) = buckets.get_mut(&key)
            && !bucket.is_empty()
        {
            bucket.remove(0);
            finding.status = Some("persisting".to_owned());
            continue;
        }
        finding.status = Some("new".to_owned());
    }
    let resolved = buckets.into_values().flatten().collect::<Vec<_>>();
    Ok(summarize(findings, file, "matched", resolved))
}

/// baselineに対応recordがないファイル用: 全findingを新規として集計する。
pub fn baseline_added(findings: &mut [Finding], file: String) -> BaselineReport {
    for finding in findings.iter_mut() {
        finding.status = Some("new".to_owned());
    }
    summarize(findings, file, "added", Vec::new())
}
