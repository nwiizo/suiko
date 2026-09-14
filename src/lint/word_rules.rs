//! プロジェクトで明示した形態素列を照合する。内蔵の語彙ルールは変更しない。

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::Error;
use crate::morphology::{Morpheme, Morphology};
use crate::text::{mask_markdown_structure_with_stats, sentences_with_raw};

use super::{Finding, morph};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WordRule {
    pub(crate) id: String,
    message: String,
    #[serde(default = "default_severity")]
    severity: String,
    tokens: Vec<TokenCondition>,
}

fn default_severity() -> String {
    "info".to_owned()
}

// nwiizo-coding-style: 条件はSudachiの3項目の完全一致だけを扱う;
// 選択肢や正規表現が必要な用例が得られたら、反例と計算量を確認して拡張する。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenCondition {
    surface: Option<String>,
    dictionary_form: Option<String>,
    pos: Option<String>,
}

impl TokenCondition {
    fn matches(&self, token: &Morpheme) -> bool {
        token.pos(0) != "空白"
            && self
                .surface
                .as_deref()
                .is_none_or(|value| token.surface == value)
            && self
                .dictionary_form
                .as_deref()
                .is_none_or(|value| token.dictionary_form() == value)
            && self
                .pos
                .as_deref()
                .is_none_or(|value| token.pos(0) == value)
    }

    fn validate(&self) -> Result<(), String> {
        let fields = [
            ("surface", self.surface.as_deref()),
            ("dictionary_form", self.dictionary_form.as_deref()),
            ("pos", self.pos.as_deref()),
        ];
        if fields.iter().all(|(_, value)| value.is_none()) {
            return Err(
                "tokens の各条件には surface / dictionary_form / pos のいずれかが必要です"
                    .to_owned(),
            );
        }
        for (name, value) in fields {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(format!("tokens.{name} は空にできません"));
            }
        }
        if let Some(pos) = &self.pos
            && !matches!(
                pos.as_str(),
                "名詞"
                    | "代名詞"
                    | "形状詞"
                    | "動詞"
                    | "形容詞"
                    | "副詞"
                    | "連体詞"
                    | "接続詞"
                    | "感動詞"
                    | "接頭辞"
                    | "接尾辞"
                    | "助詞"
                    | "助動詞"
                    | "補助記号"
                    | "記号"
            )
        {
            return Err(format!(
                "tokens.pos はSudachiの品詞大分類を指定してください: {pos}"
            ));
        }
        Ok(())
    }
}

pub(crate) fn validate_word_rules(rules: &[WordRule]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for rule in rules {
        if rule.id.is_empty()
            || !rule
                .id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
        {
            return Err(
                "word_rules.id は英小文字・数字・ハイフン・アンダースコアで指定してください"
                    .to_owned(),
            );
        }
        if !ids.insert(&rule.id) {
            return Err(format!("word_rules.id が重複しています: {}", rule.id));
        }
        if rule.message.trim().is_empty() {
            return Err(format!("word_rules[{}].message は空にできません", rule.id));
        }
        if !matches!(rule.severity.as_str(), "info" | "warn" | "critical") {
            return Err(format!(
                "word_rules[{}].severity は info / warn / critical を指定してください",
                rule.id
            ));
        }
        if rule.tokens.is_empty() {
            return Err(format!("word_rules[{}].tokens は1件以上必要です", rule.id));
        }
        for condition in &rule.tokens {
            condition
                .validate()
                .map_err(|error| format!("word_rules[{}]: {error}", rule.id))?;
        }
    }
    Ok(())
}

pub(crate) fn word_rule_findings(
    raw: &str,
    morphology: &Morphology,
    rules: &[WordRule],
) -> Result<Vec<Finding>, Error> {
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let (masked, _) = mask_markdown_structure_with_stats(raw);
    let tokenized = morph::tokenize(&sentences_with_raw(&masked, raw), morphology)?;
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let mut findings = Vec::new();
    for sentence in &tokenized {
        for rule in rules {
            for window in sentence.tokens.windows(rule.tokens.len()) {
                if !rule
                    .tokens
                    .iter()
                    .zip(window)
                    .all(|(condition, token)| condition.matches(token))
                    || !window
                        .windows(2)
                        .all(|pair| pair[0].byte_end == pair[1].byte_start)
                {
                    continue;
                }
                let Some(span) = sentence.span(
                    &raw_lines,
                    window[0].byte_start,
                    window[window.len() - 1].byte_end,
                ) else {
                    continue;
                };
                let excerpt = &raw_lines[span.start_line - 1][span.start_byte..span.end_byte];
                let mut finding = Finding::new(
                    sentence.line,
                    &format!("custom_wording/{}", rule.id),
                    excerpt,
                    &rule.severity,
                    &rule.message,
                );
                finding.span = Some(span);
                findings.push(finding);
            }
        }
    }
    Ok(findings)
}
