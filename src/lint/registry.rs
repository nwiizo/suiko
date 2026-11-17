//! 規則の登録表。規則ごとに、族・確度・範囲・評価中かどうかを1行で持つ。
//!
//! 族は直すときに書き手が決めること（docs/rule-families.md）、確度の根拠は
//! priority.rsの冒頭にある。評価中の規則は、ライブラリの`analyze(.., false)`で外し、
//! 確度をlowにする。CLIは評価中の規則も既定で表示する（ADR-0003）。

use std::sync::LazyLock;

use serde::Serialize;

use super::Confidence;
use super::Confidence::{High, Low, Medium};

/// 直すときに書き手が決めること。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// 精読したとき、何を指すかが一つに定まるか。
    Meaning,
    /// 同じ形を続ける理由があるか。
    Pattern,
    /// その語の代わりに、動作・対象・根拠を直接書けるか。
    Wording,
    /// 読点・記号・書式が、掛かり先や表示に必要か。
    Punctuation,
    /// 一文を読み終えるまでに、読み手が何を保留するか。
    ReadingLoad,
    /// 文書全体のそろい方が、内容から来ているか。
    Profile,
    /// 本文の数・条件・事実が、本文の中と資料とで食い違わないか。
    Consistency,
}

/// 指摘が本文の箇所を指すか、文書全体の統計か。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Span,
    Document,
}

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub category: &'static str,
    pub family: Family,
    pub confidence: Confidence,
    pub scope: Scope,
    /// 評価中の規則。旧版の`--experimental`でだけ出ていた。
    pub evaluating: bool,
}

const fn rule(
    category: &'static str,
    family: Family,
    confidence: Confidence,
    scope: Scope,
) -> Rule {
    Rule {
        category,
        family,
        confidence,
        scope,
        evaluating: false,
    }
}

impl Rule {
    const fn evaluating(self) -> Self {
        Self {
            evaluating: true,
            ..self
        }
    }
}

use Family::{Consistency, Meaning, Pattern, Profile, Punctuation, ReadingLoad, Wording};
use Scope::{Document, Span};

/// 規則の一覧。並びは`rule_categories()`と`capabilities --json`の順になる。
const RULES: &[Rule] = &[
    // 語の出現だけで判定するため、人間の行政文書・技術記事にも多く一致する（suiko-eval/eval/calibration.md）。
    rule("vague_loanword", Wording, Low, Span).evaluating(),
    rule("thin_evaluation", Wording, High, Span),
    rule("calque_phrase", Wording, Medium, Span),
    rule("short_concession", Wording, High, Span),
    rule("persona_ending", Wording, Medium, Span),
    rule("te_nominal_stop", Meaning, High, Span),
    rule("cleft_list_ending", Meaning, High, Span),
    rule("evaluation_fragment_run", Pattern, Medium, Span),
    rule("heading_colon", Punctuation, High, Span),
    rule("redundant_heading_bracket", Punctuation, High, Span),
    rule("metaphor_label", Wording, Medium, Span),
    rule("dialogue_period", Punctuation, Medium, Document),
    rule("essay_report_density", Profile, Medium, Document),
    rule("topic_predicate_mismatch", Meaning, Low, Span).evaluating(),
    rule("repeated_predicate_phrase", Pattern, Low, Span).evaluating(),
    rule("paragraph_register_mixture", Pattern, Low, Span).evaluating(),
    rule("word_usage", Wording, High, Span),
    rule("evaluative_ability_past", Wording, Medium, Span),
    rule("heading_emoji", Punctuation, Medium, Span),
    rule("invisible_character", Punctuation, High, Span),
    rule("invalid_ignore_directive", Consistency, High, Span),
    rule("auxiliary_kana", Wording, Medium, Span),
    rule("explanatory_metaphor", Meaning, Medium, Span),
    rule("renyo_noun", Wording, Medium, Span),
    rule("revision_expression", Wording, Medium, Span),
    rule("operation_condition", Meaning, Medium, Span),
    rule("bold_not_rendered", Punctuation, High, Span),
    rule("duplicate_comma", Punctuation, High, Span),
    rule("instruction_residue", Consistency, High, Span),
    rule("abstract_metaphor", Wording, High, Span),
    rule("antithesis_repetition", Pattern, Low, Document),
    rule("boilerplate_closing", Wording, Medium, Span),
    rule("boilerplate_heading", Pattern, Low, Span).evaluating(),
    rule("bullet_bold_label", Punctuation, Medium, Span),
    rule("bullet_emoji", Punctuation, Medium, Span),
    rule("buried_list", ReadingLoad, Medium, Span),
    rule("buried_question_list", ReadingLoad, Medium, Span),
    rule("consecutive_nominal_endings", Pattern, Low, Span).evaluating(),
    rule("copy_fragment", Meaning, High, Span),
    rule("compressed_relation", Meaning, High, Span),
    rule("custom_wording", Wording, High, Span),
    rule("decision_direction_metaphor", Wording, High, Span),
    rule("declared_item_count_mismatch", Consistency, High, Span),
    rule("demonstrative_reference", Meaning, Medium, Span),
    rule("double_negative", ReadingLoad, Medium, Span),
    // 直喩の形だけで判定するため、人間の随筆・小説にも多く一致する（suiko-eval/eval/calibration.md）。
    rule("elaborate_simile", Meaning, Low, Span).evaluating(),
    rule("english_syntax_cleft_because", Pattern, Low, Span).evaluating(),
    rule("english_syntax_inanimate_subject", Wording, Medium, Span),
    rule("forbidden_phrase", Wording, Low, Span),
    rule("high_bold_density", Profile, Low, Document).evaluating(),
    rule("high_bullet_ratio", Profile, Low, Document).evaluating(),
    rule("high_emoji_symbol_density", Profile, Low, Document).evaluating(),
    rule("high_length_autocorrelation", Profile, Low, Document).evaluating(),
    rule("hype_expression", Wording, Medium, Span),
    rule("inanimate_subject_morph", Wording, Low, Span),
    rule("kanji_run", ReadingLoad, Medium, Span),
    rule("long_attributive_span", ReadingLoad, Medium, Span),
    rule("low_burstiness", Profile, Low, Document),
    // 2026-08-19の実測(現代人間dev 75文書)で既定ONを支えられず評価中へ戻した3件。
    // TTRは文書長への構造的依存(50k語の白書でTTR=0.094)、文頭反復は絶対回数
    // 閾値の長さ交絡(fpr 0.613)、MTLDは全候補閾値でAI検出0。suiko-eval/eval/calibration.md
    rule("low_lexical_diversity_mtld", Profile, Low, Document).evaluating(),
    rule("low_lexical_diversity_ttr", Profile, Low, Document).evaluating(),
    rule("low_sentence_variance", Profile, Low, Document),
    rule("low_specificity", Wording, Medium, Span),
    rule("mixed_latin_spacing", Punctuation, Low, Span).evaluating(),
    rule("negative_listing", Pattern, Medium, Span),
    rule("no_chain", ReadingLoad, Medium, Span),
    rule("no_comma_sentence", ReadingLoad, Medium, Span),
    rule("nominal_ending", Profile, Low, Document),
    rule("numbered_phase_structure", Profile, Low, Document).evaluating(),
    rule("paragraph_lead_conjunction", Pattern, Low, Span).evaluating(),
    rule("predicate_colon_lead", Punctuation, Medium, Span),
    rule("quantity_predicate_mismatch", Consistency, Medium, Span),
    rule("quoted_character_count_mismatch", Consistency, High, Span),
    rule("redundant_light_verb", Wording, Low, Span),
    rule("repeated_distinction", Pattern, Low, Document),
    rule("repeated_em_dash", Punctuation, Low, Document),
    rule("repeated_explanation_preview", Pattern, High, Span),
    rule("repeated_sentence_lead", Pattern, Low, Span).evaluating(),
    rule("repeated_sentence_mode", Pattern, Low, Span).evaluating(),
    rule("repeated_syntax_template", Pattern, Low, Span).evaluating(),
    rule("respectively_scope", Meaning, Low, Span),
    rule("self_labeling_repetition", Pattern, Medium, Span),
    // 品質表ではkeepだが、suiko-evalの生成文評価（2026-11-06）で注釈者が自然とした擬人化
    // （光が待つ、冷たさが這い上がる）に当たり、注釈とは重ならなかった。作例と人間文書の
    // 条件だけでは確度を上げない（medium）。
    rule("sensory_role_mismatch", Meaning, Medium, Span),
    // 句点を含む括弧書き（v0.4.2で追加）。人間95文書の16文書に付き、生成文では0件だった。
    rule("sentence_in_paren", ReadingLoad, Medium, Span),
    rule("sentence_too_long", ReadingLoad, Medium, Span),
    rule("short_object_comma", Punctuation, Medium, Span),
    rule("short_topic_comma", Punctuation, Low, Span),
    rule("technical_jargon_metaphor", Wording, High, Span),
    rule("translationese", Wording, Low, Span),
    rule("translationese_morph", Wording, Low, Span),
    rule("universal_exception_conflict", Consistency, Medium, Span),
    rule("uniform_bullet_structure", Pattern, Low, Span).evaluating(),
    rule("uniform_paragraph_structure", Profile, Low, Document),
    rule("vague_sensory_term", Wording, High, Span),
    // v0.4.3で追加（docs/rule-families.mdのW2〜W4）。
    // 人間95文書の34文書に付く（suiko-eval/eval/competitive-review-2026-11-15.md）。
    rule("repeated_sentence_ending", Pattern, Low, Span),
    // 人間文書の約半数に付く（devで36/75文書）。
    rule("negated_contrast", Pattern, Low, Span),
    rule("short_subject_comma", Punctuation, Low, Span),
];

static CATEGORIES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| RULES.iter().map(|rule| rule.category).collect());

static READING_LOAD: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    RULES
        .iter()
        .filter(|rule| rule.family == ReadingLoad)
        .map(|rule| rule.category)
        .collect()
});

/// 登録された規則。`custom_wording/<id>`のような利用者の規則は、`custom_wording`の行を返す。
pub fn rule_of(category: &str) -> Option<&'static Rule> {
    let base = category.split_once('/').map_or(category, |(base, _)| base);
    RULES.iter().find(|rule| rule.category == base)
}

pub(super) fn exact(category: &str) -> Option<&'static Rule> {
    RULES.iter().find(|rule| rule.category == category)
}

pub(super) fn categories() -> &'static [&'static str] {
    &CATEGORIES
}

pub(super) fn reading_load() -> &'static [&'static str] {
    &READING_LOAD
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_rule_is_registered_once_with_one_family() {
        let mut seen = BTreeSet::new();
        for rule in RULES {
            assert!(seen.insert(rule.category), "重複: {}", rule.category);
        }
        for family in [
            Meaning,
            Pattern,
            Wording,
            Punctuation,
            ReadingLoad,
            Profile,
            Consistency,
        ] {
            assert!(RULES.iter().any(|rule| rule.family == family), "{family:?}");
        }
    }

    #[test]
    fn evaluating_rules_stay_low_confidence() {
        for rule in RULES.iter().filter(|rule| rule.evaluating) {
            assert_eq!(rule.confidence, Low, "{}", rule.category);
        }
    }

    #[test]
    fn document_level_matching_uses_document_scope() {
        for category in super::super::matching::DOCUMENT_LEVEL {
            assert_eq!(exact(category).map(|rule| rule.scope), Some(Document));
        }
    }

    #[test]
    fn custom_wording_ids_resolve_to_the_registered_rule() {
        assert_eq!(
            rule_of("custom_wording/team-term").map(|rule| rule.family),
            Some(Wording)
        );
        assert!(rule_of("unknown_rule").is_none());
    }
}
