//! たとえる側に連体節が付いた直喩を、比喩の対応を読み直す候補として示す。

use super::Finding;
use super::morph::TokenizedSentence;
use crate::morphology::Morpheme;

/// 説明や参照に使う「〜の例のように」「先に示した手順のように」は直喩ではない。
const REFERENCE_HEADS: &[&str] = &[
    "例",
    "図",
    "表",
    "場合",
    "通り",
    "結果",
    "手順",
    "方法",
    "形",
    "方",
    "事",
    "物",
    "式",
    "コード",
    "記事",
    "節",
    "章",
    "項",
    "前回",
    "以下",
    "次",
    "上記",
    "下記",
    "前述",
    "後述",
    "設定",
    "仕様",
    "説明",
    "記述",
    "定義",
    "出力",
    "画面",
    "ログ",
    "名前",
    "時",
    "頃",
    "状態",
    "様子",
    "感じ",
];

/// 「よう／みたい」の位置から、たとえる側の名詞の位置を返す。
fn vehicle_head(tokens: &[&Morpheme], marker: usize) -> Option<usize> {
    let token = tokens[marker];
    if token.pos(1) != "助動詞語幹" {
        return None;
    }
    let head = match token.surface.as_str() {
        "よう" => {
            let no = tokens.get(marker.checked_sub(1)?)?;
            if !(no.surface == "の" && no.pos(0) == "助詞") {
                return None;
            }
            marker.checked_sub(2)?
        }
        "みたい" => marker.checked_sub(1)?,
        _ => return None,
    };
    let noun = tokens[head];
    // 「最後の一滴」の「滴」は数詞に続く助数詞。
    let counter = noun.pos(0) == "接尾辞" && head > 0 && tokens[head - 1].pos(1) == "数詞";
    ((noun.pos(0) == "名詞" || counter)
        && noun.pos(1) != "数詞"
        && !REFERENCE_HEADS.contains(&noun.normalized()))
    .then_some(head)
}

/// たとえる側の名詞句の中にある、動詞を含む連体節の先頭。
fn relative_clause_start(tokens: &[&Morpheme], head: usize) -> Option<usize> {
    let mut index = head;
    while index > 0 {
        index -= 1;
        let token = tokens[index];
        // 「伴わないまま教科書のよう」の「まま」は様態を表し、たとえる名詞句の一部ではない。
        if matches!(
            token.normalized(),
            "侭" | "まま" | "為" | "ため" | "事" | "こと" | "時" | "とき"
        ) {
            return None;
        }
        let noun_phrase = matches!(token.pos(0), "名詞" | "接尾辞" | "接頭辞" | "連体詞")
            || (token.pos(0) == "形容詞" && token.pos(5).starts_with("連体形"))
            || (token.pos(0) == "助詞" && token.surface == "の");
        if noun_phrase {
            continue;
        }
        if !matches!(token.pos(0), "動詞" | "助動詞") || !token.pos(5).starts_with("連体形")
        {
            return None;
        }
        // 「生きてきた」「閉じきれなかった」の連なりを遡り、最初の動詞を節の先頭にする。
        let mut start = None;
        let mut chain = index;
        loop {
            let link = tokens[chain];
            if link.pos(0) == "動詞" {
                start = Some(chain);
            } else if !(link.pos(0) == "助動詞"
                || link.pos(1) == "接続助詞" && matches!(link.surface.as_str(), "て" | "で"))
            {
                break;
            }
            if chain == 0 {
                break;
            }
            chain -= 1;
        }
        let start = start?;
        // 「〜というXのよう」は引用・言い換えで、場面の説明ではない。
        return (tokens[start].pos(0) == "動詞" && tokens[start].normalized() != "言う")
            .then_some(start);
    }
    None
}

// nwiizo-coding-style: 「名詞＋の＋よう」「名詞＋みたい」の直前の名詞句に、動詞の連体節が
// 付く形だけを扱う。「〜するように」の目的、参照の「例のように」、形容詞だけの修飾は除く。
pub(super) fn findings(tokenized: &[TokenizedSentence], raw_lines: &[&str]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut bracket_depth: usize = 0;
    for sentence in tokenized {
        let tokens = sentence
            .tokens
            .iter()
            .filter(|token| !token.surface.chars().all(|ch| matches!(ch, '*' | '_')))
            .collect::<Vec<_>>();
        let quotes = crate::markup::quotation_ranges(&sentence.text);
        for (index, token) in tokens.iter().enumerate() {
            match token.pos(1) {
                "括弧開" => bracket_depth += 1,
                "括弧閉" => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            if bracket_depth > 0 || quotes.iter().any(|range| range.contains(&token.byte_start)) {
                continue;
            }
            let Some(head) = vehicle_head(&tokens, index) else {
                continue;
            };
            let Some(start) = relative_clause_start(&tokens, head) else {
                continue;
            };
            findings.push(sentence.info_finding(
                raw_lines,
                tokens[start].byte_start..token.byte_end,
                "elaborate_simile",
                "たとえる側に場面の説明が付いた直喩です。たとえる物と、たとえられる物のどの性質（形・温度・音・動き・時間）が対応するのかを確認してください。対応が読み取れない場合は、性質を直接書くと伝わります。意図した比喩は残せます。",
            ));
        }
    }
    findings
}
