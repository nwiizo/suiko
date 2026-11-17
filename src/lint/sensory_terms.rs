//! 語の族: 理解・根拠・関心の程度を、画質や身体感覚の語で表す言い回し（`vague_sensory_term`）。

use std::sync::LazyLock;

use regex::Regex;

use crate::morphology::Morpheme;

use super::Finding;
use super::morph::{TokenizedSentence, contains_any, token_positions};

// 画質・表示の話題では「解像度」を本来の意味で使う。
const DISPLAY_RESOLUTION_NOUNS: &[&str] = &[
    "画像",
    "画面",
    "写真",
    "動画",
    "映像",
    "ディスプレイ",
    "モニター",
    "カメラ",
    "センサー",
    "スクリーン",
    "ピクセル",
    "画素",
    "印刷",
    "レンダリング",
    "テクスチャ",
];

const RESOLUTION_PREDICATES: &[&str] = &["高い", "低い", "粗い", "上げる", "上がる", "高める"];

/// 「1920x1080」「300dpi」等の数値付き画質表記。解像度を本来の意味で使う文として除く。
static DISPLAY_UNIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d\s*(?:px|dpi|ppi|[KkPp](?:[^A-Za-z]|$))|\d\s*[x×]\s*\d")
        .expect("valid display unit regex")
});

/// 体温・熱量を測る文脈（度・カロリー）。
static MEASURED_WARMTH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9０-９一二三四五六七八九十百]\s*度|℃|カロリー|kcal|ジュール")
        .expect("valid measurement regex")
});

/// 理解・根拠・関心の程度を、画質や身体感覚の語で表す言い回し。語の出現だけでは
/// 判定せず、述語・「として」・格関係との結び付きを確認し、画質や触覚の話題を除く。
/// 業務文書や随筆にも現れるため、ジャンルを問わず確認候補にする。
pub(super) fn vague_sensory_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (sentence, index, token) in token_positions(tokenized) {
        if token.pos(0) != "名詞" {
            continue;
        }
        let tokens = &sentence.tokens;
        let hit = match token.dictionary_form() {
            "解像度" if !contains_any(tokens, DISPLAY_RESOLUTION_NOUNS)
                && !DISPLAY_UNIT.is_match(&sentence.text) =>
            {
                predicate_after_particle(tokens, index, RESOLUTION_PREDICATES).map(|end| {
                    (end, "理解や把握の程度を「解像度」で表しています。何が分かった、区別できるようになったのかを具体的に書けるか確認してください")
                })
            }
            // Sudachiは「腹落ち」を「腹」＋「落ち／落ち感」に分ける。
            "腹" if tokens
                .get(index + 1)
                .is_some_and(|next| next.surface.starts_with("落ち") && next.pos(0) == "名詞") =>
            {
                Some((index + 1, "納得を「腹落ち」で表しています。何に納得したのか、判断の根拠を書けるか確認してください"))
            }
            "肌感" | "肌" => {
                let noun_end = if token.dictionary_form() == "肌" {
                    tokens
                        .get(index + 1)
                        .filter(|next| next.dictionary_form() == "感覚")
                        .map(|_| index + 1)
                } else {
                    Some(index)
                };
                noun_end
                    .and_then(|end| basis_marker_end(tokens, end))
                    .map(|end| {
                        (end, "根拠を「肌感／肌感覚」で示しています。観察した事例、件数、期間など、判断の材料を書けるか確認してください")
                    })
            }
            "温度感" => predicate_after_particle(tokens, index, &[]).map(|end| {
                (end, "関心や優先度の程度を「温度感」で表しています。誰が、何を、どの程度急ぐのかを書けるか確認してください")
            }),
            "体温" | "熱量" if !MEASURED_WARMTH.is_match(&sentence.text)
                && !contains_any(tokens, MEASUREMENT_WORDS) =>
            {
                non_body_warmth_end(tokens, index).map(|end| {
                    (end, "言葉や組織の勢い・思いの強さを「体温」「熱量」で表しています。何がどう伝わったのか、誰がどの程度取り組んでいるのかを書けるか確認してください。体や物理量の体温・熱量なら残してください")
                })
            }
            _ => None,
        };
        let Some((end, detail)) = hit else {
            continue;
        };
        // 語の採否は原稿全体で一度判断するため、語ごとに最初の箇所へまとめて対応行を返す。
        if let Some(finding) = findings
            .iter_mut()
            .find(|finding: &&mut Finding| finding.detail == detail)
        {
            let lines = finding.related_lines.get_or_insert_with(Vec::new);
            if !lines.contains(&sentence.line) {
                lines.push(sentence.line);
            }
            continue;
        }
        let mut finding = sentence.info_finding(
            raw_lines,
            token.byte_start..tokens[end].byte_end,
            "vague_sensory_term",
            detail,
        );
        finding.related_lines = Some(vec![sentence.line]);
        findings.push(finding);
    }
    findings
}

/// 体温・熱量を持つ体や物ではないもの。
const NON_BODY_OWNERS: &[&str] = &[
    "言葉",
    "文章",
    "文字",
    "文体",
    "手紙",
    "作品",
    "物語",
    "組織",
    "チーム",
    "会社",
    "企業",
    "職場",
    "現場",
    "記憶",
    "議論",
    "会議",
    "企画",
    "提案",
    "プロジェクト",
    "文化",
    "コミュニティ",
];

const MEASUREMENT_WORDS: &[&str] = &["測る", "計る", "測定", "体温計", "平熱", "発熱"];

/// 「言葉の体温」「熱量のある文章」のように、体温・熱量を体や物理量でないものに使う範囲の終わり。
fn non_body_warmth_end(tokens: &[Morpheme], index: usize) -> Option<usize> {
    let owner = |token: &Morpheme| {
        token.pos(0) == "名詞" && NON_BODY_OWNERS.contains(&token.dictionary_form())
    };
    if index >= 2 && tokens[index - 1].surface == "の" && owner(&tokens[index - 2]) {
        return Some(index);
    }
    // 体温・熱量＋の／が＋ある・こもる・伝わる（＋た）＋名詞
    let particle = tokens.get(index + 1)?;
    let verb = tokens.get(index + 2)?;
    if !matches!(particle.surface.as_str(), "の" | "が")
        || !matches!(
            verb.dictionary_form(),
            "ある" | "こもる" | "宿る" | "伝わる"
        )
    {
        return None;
    }
    tokens
        .iter()
        .enumerate()
        .skip(index + 3)
        .take(2)
        .find(|(_, token)| owner(token))
        .map(|(end, _)| end)
}

/// 名詞＋が／を／は／も／に＋（程度の語）＋述語。候補が空なら、動詞・形容詞・サ変名詞＋するを述語とみなす。
/// 「一段」「格段に」「さらに」のような程度の語は3語まで読み飛ばす。
fn predicate_after_particle(
    tokens: &[Morpheme],
    noun: usize,
    predicates: &[&str],
) -> Option<usize> {
    let particle = tokens.get(noun + 1)?;
    if particle.pos(0) != "助詞"
        || !matches!(particle.surface.as_str(), "が" | "を" | "は" | "も" | "に")
    {
        return None;
    }
    let degree = |token: &Morpheme| {
        matches!(token.pos(0), "副詞" | "形状詞" | "接尾辞")
            || (token.pos(0) == "名詞" && matches!(token.pos(1), "数詞"))
            || (token.pos(0) == "名詞" && matches!(token.pos(2), "助数詞可能" | "副詞可能"))
            || (token.pos(0) == "助動詞" && token.surface == "に")
    };
    let index = (noun + 2..tokens.len().min(noun + 6))
        .find(|index| !degree(&tokens[*index]))
        .filter(|index| *index <= noun + 5)?;
    let predicate = &tokens[index];
    if !predicates.is_empty() {
        return predicates
            .contains(&predicate.dictionary_form())
            .then_some(index);
    }
    match predicate.pos(0) {
        "動詞" | "形容詞" => Some(index),
        "名詞"
            if tokens
                .get(index + 1)
                .is_some_and(|next| next.dictionary_form() == "する") =>
        {
            Some(index + 1)
        }
        _ => None,
    }
}

/// 名詞を判断の根拠として示す「として／では／で／的に」。
fn basis_marker_end(tokens: &[Morpheme], noun: usize) -> Option<usize> {
    let marker = tokens.get(noun + 1)?;
    match marker.surface.as_str() {
        "として" => Some(noun + 1),
        "と" if tokens
            .get(noun + 2)
            .is_some_and(|next| next.dictionary_form() == "する")
            && tokens
                .get(noun + 3)
                .is_some_and(|next| next.surface == "て") =>
        {
            Some(noun + 3)
        }
        "で" if marker.pos(0) == "助詞" => Some(noun + 1),
        "的" => Some(noun + 1),
        _ => None,
    }
}
