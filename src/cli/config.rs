//! `.suiko.toml`の読み込みと、設定（外す規則・理由付きの許可・語の規則）を適用したlint。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::lint::Finding;
use crate::morphology::Morphology;
use crate::{Error, lint, read_source};

use super::{FailOn, Genre};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    version: u32,
    pub(super) genre: Option<Genre>,
    pub(super) fail_on: Option<FailOn>,
    #[serde(default)]
    disabled_rules: Vec<String>,
    #[serde(default)]
    allow: Vec<Allowance>,
    #[serde(default)]
    word_rules: Vec<lint::WordRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowance {
    category: String,
    rule_id: Option<String>,
    text: String,
    reason: String,
}

impl Config {
    fn validate(&self, path: &Path) -> Result<(), Error> {
        if self.version != 1 {
            return Err(config_error(
                path,
                format!(
                    "version = {} は未対応です。version = 1 を指定してください",
                    self.version
                ),
            ));
        }
        for rule in &self.disabled_rules {
            if !lint::is_known_rule(rule) {
                return Err(config_error(path, format!("未知のルールです: {rule}")));
            }
        }
        for allowance in &self.allow {
            if !lint::is_known_rule(&allowance.category) {
                return Err(config_error(
                    path,
                    format!("未知のルールです: {}", allowance.category),
                ));
            }
            if allowance.text.trim().is_empty() {
                return Err(config_error(path, "allow.text は空にできません"));
            }
            if allowance.reason.trim().is_empty() {
                return Err(config_error(path, "allow.reason は空にできません"));
            }
            if let Some(rule_id) = &allowance.rule_id
                && (allowance.category != "custom_wording"
                    || !self.word_rules.iter().any(|rule| rule.id == *rule_id))
            {
                return Err(config_error(
                    path,
                    format!(
                        "allow.rule_id は custom_wording の登録済みIDを指定してください: {rule_id}"
                    ),
                ));
            }
        }
        lint::validate_word_rules(&self.word_rules)
            .map_err(|message| config_error(path, message))?;
        Ok(())
    }

    fn suppresses(&self, finding: &Finding) -> bool {
        let (category, rule_id) = finding
            .category
            .strip_prefix("custom_wording/")
            .map_or((finding.category.as_str(), None), |id| {
                ("custom_wording", Some(id))
            });
        self.disabled_rules.iter().any(|rule| rule == category)
            || self.allow.iter().any(|allowance| {
                allowance.category == category
                    && allowance
                        .rule_id
                        .as_deref()
                        .is_none_or(|id| rule_id == Some(id))
                    && finding.excerpt.contains(&allowance.text)
            })
    }
}

fn config_error(path: &Path, message: impl Into<String>) -> Error {
    Error::Config {
        path: path.display().to_string(),
        message: message.into(),
    }
}

pub(super) fn load_config(
    explicit: Option<&Path>,
    no_config: bool,
) -> Result<Option<Config>, Error> {
    let Some(path) = config_path(explicit, no_config)? else {
        return Ok(None);
    };
    if explicit.is_none()
        && !path
            .try_exists()
            .map_err(|e| config_error(&path, e.to_string()))?
    {
        return Ok(None);
    }
    let source = read_source(&path)?;
    parse_config(&path, &source).map(Some)
}

pub(super) fn config_path(
    explicit: Option<&Path>,
    no_config: bool,
) -> Result<Option<PathBuf>, Error> {
    if no_config {
        return Ok(None);
    }
    explicit.map(Path::to_path_buf).map_or_else(
        || {
            std::env::current_dir()
                .map(|dir| Some(dir.join(".suiko.toml")))
                .map_err(|source| config_error(Path::new(".suiko.toml"), source.to_string()))
        },
        |path| Ok(Some(path)),
    )
}

pub(super) fn parse_config(path: &Path, source: &str) -> Result<Config, Error> {
    let config = toml::from_str::<Config>(source)
        .map_err(|source| config_error(path, source.to_string()))?;
    config.validate(path)?;
    Ok(config)
}

pub(super) fn configured_lint(
    text: &str,
    morphology: &Morphology,
    genre: Option<&str>,
    experimental: bool,
    config: Option<&Config>,
) -> Result<lint::LintReport, Error> {
    let mut report = lint::analyze(text, morphology, genre, experimental)?;
    if let Some(config) = config {
        report.findings.extend(lint::word_rule_findings(
            text,
            morphology,
            &config.word_rules,
        )?);
        report.findings.sort_by_key(|finding| finding.line);
    }
    apply_config(&mut report, config);
    Ok(report)
}

fn category_counts(findings: &[Finding]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for finding in findings {
        *counts.entry(finding.category.clone()).or_default() += 1;
    }
    counts
}

/// 設定による除外を適用し、残ったfindingの総数とカテゴリ別内訳を返す。
fn retain_allowed(
    findings: &mut Vec<Finding>,
    config: &Config,
) -> (usize, BTreeMap<String, usize>) {
    findings.retain(|finding| !config.suppresses(finding));
    (findings.len(), category_counts(findings))
}

pub(super) fn apply_config(report: &mut lint::LintReport, config: Option<&Config>) {
    if let Some(config) = config {
        let (total, by_category) = retain_allowed(&mut report.findings, config);
        report.stats.total_findings = total;
        report.stats.by_category = by_category;
    }
}

pub(super) fn apply_reading_load_config(
    report: &mut lint::ReadingLoadReport,
    config: Option<&Config>,
) {
    if let Some(config) = config {
        let (total, by_category) = retain_allowed(&mut report.findings, config);
        report.stats.total = total;
        report.stats.by_category = by_category;
    }
}
