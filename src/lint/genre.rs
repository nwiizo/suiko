//! 形態素の分布から文書の種類（随筆・技術・業務）を推定する。
//! 仮説: 種類の違いは語彙ではなく、品詞・人称・数詞・文末の分布に表れる。
//! 推定は既定の閾値を選ぶための手がかりで、利用者が指定した --genre を上書きしない。

use serde::Serialize;
use serde_json::{Value, json};

use super::morph::TokenizedSentence;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct GenreFeatures {
    pub chars: usize,
    /// 千字あたりの一人称。
    pub first_person: f64,
    /// 千字あたりの数詞。
    pub numerals: f64,
    /// 内容語のうち英字の語の割合。
    pub latin: f64,
    /// 普通名詞のうちサ変可能な漢語名詞の割合。
    pub sahen: f64,
    /// 敬体で終わる文の割合。
    pub polite: f64,
    /// 「た」で終わる文の割合。
    pub past: f64,
    /// 千字あたりの副詞。
    pub adverbs: f64,
    /// 千字あたりのコード（フェンス行とインラインコード）。
    pub code: f64,
}

const FIRST_PERSON: &[&str] = &[
    "私",
    "僕",
    "俺",
    "我",
    "我々",
    "わたくし",
    "自分",
    "筆者",
    "あたし",
    "うち",
];

fn sentence_end(sentence: &TokenizedSentence) -> Vec<&str> {
    sentence
        .tokens
        .iter()
        .rev()
        .filter(|token| !matches!(token.pos(0), "補助記号" | "記号" | "空白"))
        .take(3)
        .map(|token| token.normalized())
        .collect()
}

pub(crate) fn features(tokenized: &[TokenizedSentence], raw: &str) -> GenreFeatures {
    let chars = tokenized
        .iter()
        .map(|sentence| {
            sentence
                .text
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .count()
        })
        .sum::<usize>()
        .max(1);
    let per_mille = |count: usize| count as f64 * 1000.0 / chars as f64;
    let ratio = |part: usize, whole: usize| {
        if whole == 0 {
            0.0
        } else {
            part as f64 / whole as f64
        }
    };
    let tokens = tokenized.iter().flat_map(|sentence| &sentence.tokens);
    let mut first_person = 0;
    let mut numerals = 0;
    let mut content = 0;
    let mut latin = 0;
    let mut common_nouns = 0;
    let mut sahen = 0;
    let mut adverbs = 0;
    for token in tokens {
        let pos = token.pos(0);
        if (pos == "代名詞" || pos == "名詞") && FIRST_PERSON.contains(&token.normalized()) {
            first_person += 1;
        }
        if token.pos(1) == "数詞" {
            numerals += 1;
        }
        if matches!(pos, "名詞" | "動詞" | "形容詞" | "形状詞" | "副詞") {
            content += 1;
            if token.surface.chars().all(|ch| ch.is_ascii_alphabetic()) {
                latin += 1;
            }
        }
        if pos == "名詞" && token.pos(1) == "普通名詞" {
            common_nouns += 1;
            if token.pos(2) == "サ変可能" {
                sahen += 1;
            }
        }
        if pos == "副詞" {
            adverbs += 1;
        }
    }
    let ends = tokenized.iter().map(sentence_end).collect::<Vec<_>>();
    let judged = ends.iter().filter(|end| !end.is_empty()).count();
    let polite = ends
        .iter()
        .filter(|end| end.iter().any(|word| matches!(*word, "です" | "ます")))
        .count();
    let past = ends.iter().filter(|end| end.first() == Some(&"た")).count();
    let fenced = raw
        .lines()
        .filter(|line| line.trim_start().starts_with("```"))
        .count();
    let inline = raw.matches('`').count() / 2;
    GenreFeatures {
        chars,
        first_person: per_mille(first_person),
        numerals: per_mille(numerals),
        latin: ratio(latin, content),
        sahen: ratio(sahen, common_nouns),
        polite: ratio(polite, judged),
        past: ratio(past, judged),
        adverbs: per_mille(adverbs),
        code: per_mille(fenced + inline),
    }
}

/// 閾値は外部コーパス94文書（業務28・技術45・随筆21、ファイル名の種類を正解とする）で
/// 80/94が一致した決定順。標本内で選んだ値なので、汎化の精度ではない（suiko-eval/eval/calibration.md）。
pub(crate) fn infer(features: &GenreFeatures) -> &'static str {
    if features.code >= 0.5 || features.latin >= 0.27 {
        // コードや英字の語が多い文書は、文体によらず技術文書。
        "tech"
    } else if features.latin < 0.05 && features.adverbs >= 7.5 {
        // 副詞（ふと、まるで、やはり）の多さは、随筆の語りの強い手がかり。
        "essay"
    } else if features.numerals >= 12.0 && features.polite < 0.6 {
        // 数詞が多く常体の文書は、白書・報告などの業務文書。
        "business"
    } else if features.latin >= 0.03 {
        "tech"
    } else if features.sahen >= 0.25 {
        // 漢語のサ変名詞（実施、対応、推進）が多い。
        "business"
    } else {
        "essay"
    }
}

/// 利用者が auto を指定したときだけ推定した種類を使う。
pub(crate) fn resolve<'a>(genre: Option<&'a str>, features: &GenreFeatures) -> Option<&'a str> {
    match genre {
        Some("auto") => Some(infer(features)),
        other => other,
    }
}

pub(crate) fn observation(features: &GenreFeatures, applied: Option<&str>) -> Value {
    json!({"inferred": infer(features), "applied": applied, "features": features})
}
