//! 物・感覚・時間・性質を、意図や身体を持つ動作主として扱う組み合わせを指す。

use super::Finding;
use super::morph::TokenizedSentence;
use crate::morphology::Morpheme;

#[derive(Clone, Copy, PartialEq)]
enum Agent {
    /// 家具・器・道具など、自分では動かない物。
    Object,
    /// 光・音・匂い・空気など、知覚される現象。
    Sense,
    /// 午後・夜など、時間の区切り。
    Time,
    /// 冷たさ・白さなど、物の性質。
    Property,
}

fn agent(tokens: &[&Morpheme], index: usize) -> Option<(Agent, usize)> {
    let token = tokens[index];
    if token.pos(0) == "形容詞"
        && matches!(
            token.normalized(),
            "冷たい" | "温かい" | "暖かい" | "白い" | "甘い"
        )
        && tokens
            .get(index + 1)
            .is_some_and(|next| next.pos(0) == "接尾辞" && next.surface == "さ")
    {
        return Some((Agent::Property, index + 1));
    }
    if token.pos(0) != "名詞" || token.pos(1) != "普通名詞" {
        return None;
    }
    // 自分では動かない家具・食器・台所用品・文具・持ち物。首を振る扇風機や、物を受け止める
    // 壁・床・網のように字義どおりに動作し得る物は入れない。
    let kind = match token.normalized() {
        "窓" | "窓ガラス" | "鏡" | "靴べら" | "ハンガー" | "リモコン" | "換気扇" | "背表紙"
        | "湯呑み" | "湯飲み" | "布" | "ベンチ" | "棚" | "皿" | "器" | "机" | "街灯" | "電球"
        | "傘" | "カップ" | "缶" | "扉" | "戸" | "ドア" | "椅子" | "ソファ" | "ソファー"
        | "テーブル" | "本棚" | "食器棚" | "箪笥" | "タンス" | "引き出し" | "ベッド" | "枕"
        | "鍋" | "釜" | "薬缶" | "やかん" | "フライパン" | "茶碗" | "急須" | "コップ"
        | "グラス" | "瓶" | "箸" | "スプーン" | "鞄" | "かばん" | "財布" | "靴" | "靴箱"
        | "万年筆" | "鉛筆" | "ノート" | "手帳" | "ランプ" => Agent::Object,
        "光" | "灯り" | "明かり" | "空気" | "気配" | "香り" | "匂い" | "影" => {
            Agent::Sense
        }
        "午後" | "夜" | "朝" | "昼間" | "夕方" | "一日" => Agent::Time,
        "温もり" | "ぬくもり" | "凹凸" | "冷気" => Agent::Property,
        _ => return None,
    };
    Some((kind, index))
}

/// 動作主の後の助詞。「だけ」「のほう」を一つ挟める。
fn subject_marker(tokens: &[&Morpheme], end: usize) -> Option<usize> {
    let mut index = end + 1;
    if tokens.get(index).is_some_and(|t| t.surface == "だけ") {
        index += 1;
    } else if tokens.get(index).is_some_and(|t| t.surface == "の")
        && tokens
            .get(index + 1)
            .is_some_and(|t| matches!(t.normalized(), "方" | "ほう"))
    {
        index += 2;
    }
    let marker = tokens.get(index)?;
    (marker.pos(0) == "助詞" && matches!(marker.surface.as_str(), "が" | "は")).then_some(index)
}

fn negated(tokens: &[&Morpheme], verb: usize) -> bool {
    tokens[verb + 1..]
        .iter()
        .take_while(|t| {
            t.pos(0) == "助動詞"
                || (t.pos(0) == "動詞" && t.pos(1) == "非自立可能")
                || (t.pos(1) == "接続助詞" && matches!(t.surface.as_str(), "て" | "で"))
        })
        .any(|t| matches!(t.normalized(), "ない" | "無い" | "ぬ" | "ず" | "ん"))
}

/// 動詞の連体形の連なりが名詞を修飾していれば、その連なりの終わり。
fn relative_clause_end(tokens: &[&Morpheme], verb: usize) -> Option<usize> {
    let mut end = verb;
    while tokens.get(end + 1).is_some_and(|t| {
        t.pos(0) == "助動詞"
            || (t.pos(0) == "動詞" && t.pos(1) == "非自立可能")
            || (t.pos(1) == "接続助詞" && matches!(t.surface.as_str(), "て" | "で"))
    }) {
        end += 1;
    }
    (tokens[end].pos(5).starts_with("連体形")
        && tokens.get(end + 1).is_some_and(|t| t.pos(0) == "名詞"))
    .then_some(end)
}

/// 主語の後で、連体節を飛ばした最初の述語の動詞。別の主語・句点・引用で止める。
fn main_verb(tokens: &[&Morpheme], start: usize) -> Option<usize> {
    let mut index = start;
    while index < tokens.len().min(start + 30) {
        let token = tokens[index];
        match token.pos(0) {
            // 「靴下を通して」は経路を示す助詞相当の句で、述語ではない。
            "動詞"
                if token.normalized() == "通す"
                    && index > 0
                    && tokens[index - 1].surface == "を"
                    && tokens.get(index + 1).is_some_and(|t| t.surface == "て") =>
            {
                index += 2;
            }
            "動詞" => match relative_clause_end(tokens, index) {
                Some(end) => index = end + 1,
                None => return Some(index),
            },
            // 「ゆっくりと」「ぼんやりと」の「と」は副詞の一部で、引用や並列ではない。
            "助詞"
                if token.surface == "と" && index > start && tokens[index - 1].pos(0) == "副詞" =>
            {
                index += 1;
            }
            "助詞" if matches!(token.surface.as_str(), "が" | "は" | "と" | "って") => {
                // 連体節の中の「が」（シャッターの閉じた／湯気の立つ）は越えない。
                return None;
            }
            "補助記号" if token.pos(1) != "読点" => return None,
            _ => index += 1,
        }
    }
    None
}

fn mismatched_action(tokens: &[&Morpheme], kind: Agent, verb: usize) -> Option<&'static str> {
    let action = tokens[verb].normalized();
    let previous = verb.checked_sub(2).map(|i| tokens[i].normalized());
    let object_is =
        |noun: &str| verb >= 2 && tokens[verb - 1].surface == "を" && previous == Some(noun);
    let asleep = matches!(action, "就く" | "付く")
        && verb >= 2
        && tokens[verb - 1].surface == "に"
        && previous == Some("眠り");
    match kind {
        Agent::Object
            if asleep
                || object_is("首") && action == "振る"
                || matches!(
                    action,
                    "待つ"
                        | "見送る"
                        | "抱える"
                        | "抱く"
                        | "受け止める"
                        | "留める"
                        | "鳴らす"
                        | "唸る"
                        | "戻す"
                        | "眠る"
                        | "探す"
                ) =>
        {
            Some("explanatory_metaphor")
        }
        Agent::Sense
            if asleep
                || matches!(
                    action,
                    "見送る" | "待つ" | "起きる" | "拭き取る" | "受け止める" | "抱える" | "眠る"
                ) =>
        {
            Some("sensory_role_mismatch")
        }
        Agent::Time if matches!(action, "揺れる" | "拭き取る" | "見送る") => {
            Some("sensory_role_mismatch")
        }
        Agent::Property
            if matches!(
                action,
                "這う"
                    | "這い上がる"
                    | "上る"
                    | "昇る"
                    | "登る"
                    | "漂う"
                    | "包む"
                    | "入り込む"
                    | "揃える"
                    | "乗る"
            ) =>
        {
            Some("sensory_role_mismatch")
        }
        _ => None,
    }
}

fn detail(kind: Agent) -> &'static str {
    match kind {
        Agent::Object => {
            "物が、意図や身体を持つ人のように動作しています。実際に動いた物や人と、物の様子を区別できるか確認してください。意図した擬人化は残し、誰の動作かを推測で補わないでください。"
        }
        Agent::Sense => {
            "光・音・匂い・空気が、人のように見送る・待つ・起きる等の動作をしています。光や音を出す物と、それを受け取った人の感じ方を区別できるか確認してください。意図した擬人化は残せます。"
        }
        Agent::Time => {
            "時間の区切りが、物のように揺れる・拭き取る等の動作をしています。実際に動いた光や物と、時間の経過を区別できるか確認してください。意図した比喩は残せます。"
        }
        Agent::Property => {
            "冷たさ・温もり・凹凸のような性質が、物のように移動したり包んだりしています。熱を運ぶ空気や物、触れている面と、そこから感じる性質を区別できるか確認してください。意図した比喩は残せます。"
        }
    }
}

// nwiizo-coding-style: 動作主の語の類と、その類が通常は取らない動詞の組だけを照合する;
// 係り受けは「連体節を飛ばした最初の述語」までに限り、別の主語や句点を越えない。
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
            let Some((kind, end)) = agent(&tokens, index) else {
                continue;
            };
            let Some(marker) = subject_marker(&tokens, end) else {
                continue;
            };
            let Some(verb) = main_verb(&tokens, marker + 1) else {
                continue;
            };
            if negated(&tokens, verb) {
                continue;
            }
            let Some(category) = mismatched_action(&tokens, kind, verb) else {
                continue;
            };
            findings.push(sentence.info_finding(
                raw_lines,
                token.byte_start..tokens[verb].byte_end,
                category,
                detail(kind),
            ));
        }
    }
    findings
}
