//! 提供資料との照合候補。値の出現と事実の裏付けを区別し、対象や留保も確認する。
//! 外部の事実検索や、意味の同一性・資料の正しさの判定は行わない。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::{Error, diff, morphology::Morphology, text};

pub struct Source<'a> {
    pub path: PathBuf,
    pub text: &'a str,
}

#[derive(Debug, Serialize)]
pub struct Evidence {
    pub path: PathBuf,
    pub line: usize,
    pub excerpt: String,
}

#[derive(Debug, Serialize)]
pub struct MaterialFinding {
    pub category: String,
    pub severity: String,
    pub line: usize,
    pub excerpt: String,
    pub detail: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Serialize)]
pub struct MaterialReport {
    pub source_count: usize,
    pub checked_values: usize,
    pub checked_claims: usize,
    pub findings: Vec<MaterialFinding>,
}

struct Statement {
    line: usize,
    text: String,
    nouns: BTreeSet<String>,
    values: BTreeSet<String>,
    facets: [BTreeSet<&'static str>; 3],
    caveats: BTreeSet<usize>,
    claims: BTreeSet<usize>,
    negated: bool,
}

// 語彙の役割ごとに分類し、値を見つけただけで別の環境・測定値の根拠にしない。
const FACETS: &[&[(&str, &str)]] = &[
    &[
        ("本番", "本番"),
        ("検証環境", "検証"),
        ("ステージング", "検証"),
        ("テスト環境", "テスト"),
        ("開発環境", "開発"),
    ],
    &[
        ("中央値", "中央値"),
        ("平均", "平均"),
        ("最大", "最大"),
        ("最小", "最小"),
        ("上限", "上限"),
        ("下限", "下限"),
        ("p95", "p95"),
        ("p99", "p99"),
    ],
    &[
        ("成功時", "成功"),
        ("失敗時", "失敗"),
        ("変更前", "変更前"),
        ("変更後", "変更後"),
    ],
];

static CAVEATS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    patterns(&[
        r"未測定|測定(?:していません|していない|しなかった|されていない|されていません)",
        r"未確認|確認(?:していません|していない|できていない|できていません)",
        r"未検証|検証(?:していません|していない|されていない|されていません)",
        r"未定|決まってい(?:ない|ません)",
        r"不明|分かってい(?:ない|ません)|わかってい(?:ない|ません)",
        r"調査中|調査してい(?:る|ます)",
        r"予定|計画中",
        r"可能性|かもしれ",
        r"のみ|だけ|に限り|の場合|の場合に限",
    ])
});

static CLAIMS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    patterns(&[
        r"必ず|絶対に|確実に|保証",
        r"常に|すべて|全て|一切",
        r"改善|向上|高速化|短縮|削減|解消|防止",
        r"安全|安定|最適|最速|唯一",
    ])
});

fn patterns(items: &[&str]) -> Vec<Regex> {
    items
        .iter()
        .map(|pattern| Regex::new(pattern).expect("valid material cue"))
        .collect()
}

fn marks(text: &str, patterns: &[Regex]) -> BTreeSet<usize> {
    patterns
        .iter()
        .enumerate()
        .filter_map(|(index, pattern)| pattern.is_match(text).then_some(index))
        .collect()
}

fn statements(raw: &str, morphology: &Morphology) -> Result<Vec<Statement>, Error> {
    let visible = text::revision_text(raw);
    text::sentences_with_raw(&visible, raw)
        .into_iter()
        .map(|sentence| {
            let tokens = morphology.tokenize(&sentence.text)?;
            let nouns = tokens
                .iter()
                .filter(|token| token.pos(0) == "名詞" && token.pos(1) != "数詞")
                .map(|token| token.normalized())
                .filter(|word| {
                    (word.chars().count() >= 2 || word.chars().any(|ch| ch.is_ascii_alphabetic()))
                        && !matches!(
                            *word,
                            "こと" | "もの" | "ため" | "今回" | "結果" | "必要" | "場合" | "環境"
                        )
                })
                .map(str::to_owned)
                .collect();
            let facets = std::array::from_fn(|index| {
                FACETS[index]
                    .iter()
                    .filter_map(|(word, canonical)| {
                        sentence.text.contains(word).then_some(*canonical)
                    })
                    .collect()
            });
            Ok(Statement {
                line: sentence.line,
                nouns,
                facets,
                values: diff::numbers(&sentence.text).into_iter().collect(),
                caveats: marks(&sentence.text, &CAVEATS),
                claims: marks(&sentence.text, &CLAIMS),
                negated: tokens.iter().any(|token| {
                    token.pos(0) == "助動詞"
                        && matches!(token.dictionary_form(), "ない" | "ぬ" | "ず")
                }),
                text: sentence.text,
            })
        })
        .collect()
}

fn related(left: &Statement, right: &Statement) -> bool {
    !left.nouns.is_disjoint(&right.nouns)
}

fn same_context(left: &Statement, right: &Statement) -> bool {
    left.facets == right.facets
        && left.nouns.is_subset(&right.nouns)
        && left.negated == right.negated
}

fn excerpt(text: &str) -> String {
    text.trim().chars().take(160).collect()
}

fn evidence(source: &Source<'_>, statement: &Statement) -> Evidence {
    Evidence {
        path: source.path.clone(),
        line: statement.line,
        excerpt: excerpt(&statement.text),
    }
}

fn finding(
    statement: &Statement,
    category: &str,
    severity: &str,
    detail: String,
    evidence: Vec<Evidence>,
) -> MaterialFinding {
    MaterialFinding {
        category: category.to_owned(),
        severity: severity.to_owned(),
        line: statement.line,
        excerpt: excerpt(&statement.text),
        detail,
        evidence,
    }
}

pub fn compare(
    final_text: &str,
    sources: &[Source<'_>],
    morphology: &Morphology,
) -> Result<MaterialReport, Error> {
    let final_statements = statements(final_text, morphology)?;
    let source_statements = sources
        .iter()
        .map(|source| statements(source.text, morphology))
        .collect::<Result<Vec<_>, _>>()?;
    let rows = sources
        .iter()
        .zip(&source_statements)
        .flat_map(|(source, statements)| {
            statements.iter().map(move |statement| (source, statement))
        })
        .collect::<Vec<_>>();
    let mut report = MaterialReport {
        source_count: sources.len(),
        checked_values: 0,
        checked_claims: 0,
        findings: Vec::new(),
    };
    for statement in &final_statements {
        for value in &statement.values {
            report.checked_values += 1;
            let matching = rows
                .iter()
                .filter(|(_, row)| row.values.contains(value))
                .collect::<Vec<_>>();
            let (category, detail, candidates) = if matching.is_empty() {
                (
                    "material_value_unmatched",
                    format!(
                        "「{value}」と同じ値・単位を資料で確認できません。対象・単位・時点を読み合わせてください。誤りと断定した結果ではありません。"
                    ),
                    rows.iter()
                        .filter(|(_, row)| related(statement, row))
                        .collect::<Vec<_>>(),
                )
            } else if !matching.iter().any(|(_, row)| same_context(statement, row)) {
                (
                    "material_value_context",
                    format!(
                        "「{value}」は資料にありますが、環境・測定指標・条件または対象の対応を確認できません。同じ数字だけを根拠にしないでください。"
                    ),
                    matching,
                )
            } else {
                continue;
            };
            report.findings.push(finding(
                statement,
                category,
                "warn",
                detail,
                candidates
                    .iter()
                    .take(3)
                    .map(|(source, row)| evidence(source, row))
                    .collect(),
            ));
        }
        if !statement.claims.is_empty() {
            report.checked_claims += 1;
            let supported = rows.iter().any(|(_, row)| {
                same_context(statement, row)
                    && statement.claims.is_subset(&row.claims)
                    && statement.values.is_subset(&row.values)
            });
            if !supported {
                report.findings.push(finding(statement, "material_claim_unmatched", "info", "効果・保証・一般化の表現に対応する記述を資料で確認できません。根拠のある範囲と確度で書き、資料にない効果を推測で足さないでください。".to_owned(), rows.iter().filter(|(_, row)| related(statement, row)).take(3).map(|(source, row)| evidence(source, row)).collect()));
            }
        }
    }
    for (source, row) in &rows {
        if row.caveats.is_empty() {
            continue;
        }
        let related_final = final_statements
            .iter()
            .filter(|statement| related(statement, row))
            .collect::<Vec<_>>();
        if !related_final.is_empty()
            && !related_final.iter().any(|statement| {
                row.caveats.is_subset(&statement.caveats) && statement.facets == row.facets
            })
        {
            let target = related_final
                .iter()
                .max_by_key(|statement| statement.nouns.intersection(&row.nouns).count())
                .expect("nonempty related statements");
            report.findings.push(finding(target, "material_caveat", "warn", "関連する資料に未確認事項・予定・限定条件があります。本文で同じ留保を確認できないため、確定した事実や無条件の説明へ変えていないか読み合わせてください。".to_owned(), vec![evidence(source, row)]));
        }
    }
    report
        .findings
        .sort_by(|left, right| (left.line, &left.category).cmp(&(right.line, &right.category)));
    Ok(report)
}
