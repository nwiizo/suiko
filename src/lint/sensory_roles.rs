//! 感覚語と述語の限られた組み合わせを、比喩の正誤を断定せず指さす。

use super::Finding;
use super::morph::TokenizedSentence;
use super::relations::{Predicate, active_predicate, first_predicate};
use crate::morphology::Morpheme;

#[derive(Clone, Copy)]
enum SensoryRole {
    Sound,
    NonVisual,
    Silence,
    VisibleBody,
    /// 測った値としての温度。熱や空気のように移動・接触する物ではない。
    Temperature,
}

// nwiizo-coding-style: 語彙と助詞で絞れる3つの型だけを扱い、係り受けや意味全般は推定しない;
// 新しい用例を加える際は、自然な組み合わせと別の節へ係る反例を一緒に検証する。
fn sensory_argument(tokens: &[&Morpheme], index: usize) -> Option<(SensoryRole, usize)> {
    let token = tokens[index];
    let role = if token.pos(0) == "名詞" && token.pos(1) == "普通名詞" {
        match token.normalized() {
            "音" | "雨音" | "足音" | "物音" | "響き" | "声" | "音色" | "羽音" | "靴音"
            | "電子音" => SensoryRole::Sound,
            "匂い" | "臭い" | "におい" | "香り" | "温もり" | "ぬくもり" | "温かさ" | "甘味"
            | "苦味" | "酸味" | "渋味" | "塩味" | "甘み" | "苦み" | "渋み" | "湿り気"
            | "手触り" => SensoryRole::NonVisual,
            "静けさ" | "静寂" => SensoryRole::Silence,
            "背中" | "横顔" | "後ろ姿" | "表情" | "顔色" => SensoryRole::VisibleBody,
            // 「気温」は天候の説明で動作主として使われやすいため含めない。
            "温度" => SensoryRole::Temperature,
            _ => return None,
        }
    } else if token.pos(0) == "形容詞"
        && matches!(
            token.normalized(),
            "冷たい"
                | "温かい"
                | "暖かい"
                | "熱い"
                | "甘い"
                | "苦い"
                | "酸っぱい"
                | "渋い"
                | "柔らかい"
                | "硬い"
        )
        && tokens
            .get(index + 1)
            .is_some_and(|next| next.pos(0) == "接尾辞" && next.surface == "さ")
    {
        return Some((SensoryRole::NonVisual, index + 1));
    } else {
        return None;
    };
    Some((role, index))
}

fn physical_surface(token: &Morpheme) -> bool {
    token.pos(0) == "名詞"
        && matches!(
            token.normalized(),
            "窓" | "窓ガラス"
                | "ガラス"
                | "ガラス戸"
                | "戸"
                | "扉"
                | "ドア"
                | "門"
                | "壁"
                | "床"
                | "板"
                | "天井"
                | "屋根"
                | "机"
        )
}

fn candidate_predicate(
    tokens: &[&Morpheme],
    argument_end: usize,
    role: SensoryRole,
) -> Option<(usize, &'static str)> {
    let marker = tokens.get(argument_end + 1)?;
    if marker.pos(0) != "助詞" {
        return None;
    }
    let subject = matches!(marker.surface.as_str(), "が" | "は");
    let object = marker.surface == "を";
    let predicate = first_predicate(tokens, argument_end + 2, |token| match role {
        SensoryRole::Sound => {
            physical_surface(token) || matches!(token.normalized(), "香り" | "匂い" | "臭い")
        }
        SensoryRole::NonVisual if subject => {
            matches!(token.normalized(), "光" | "照明" | "日光" | "明かり")
        }
        // 「温度が裾を暖める」のように、温める・冷やす対象を一つだけ許す。
        SensoryRole::Temperature if subject => token.pos(0) == "名詞",
        _ => false,
    })?;
    let verb = tokens[predicate.head].normalized();
    let detail = match role {
        SensoryRole::Sound if subject => {
            if let Some(object) = predicate.object {
                if physical_surface(tokens[object]) && matches!(verb, "叩く" | "押す" | "揺らす")
                {
                    "音が物を動かす主語になっています。動いた物と、その結果生じた音を取り違えていないか確認してください。意図した比喩は残せます。"
                } else if matches!(tokens[object].normalized(), "香り" | "匂い" | "臭い")
                    && verb == "運ぶ"
                {
                    "音が匂いを運ぶ主語になっています。空気や物の動きと、その結果聞こえる音を区別できるか確認してください。意図した共感覚は残せます。"
                } else {
                    return None;
                }
            } else {
                let next = continued_predicate(tokens, &predicate)?;
                return Some((
                    next,
                    "音を主語にした動作のあとに、物の開閉や破損が続いています。同じ主語が続くのか、別の物の動作へ移ったのか確認してください。省略された主語を推測で補わないでください。",
                ));
            }
        }
        SensoryRole::NonVisual if object && matches!(verb, "見詰める" | "眺める") => {
            "温度・匂い・味・触感が視覚動詞の目的語になっています。見ている物と、そこから感じる性質を区別できるか確認してください。意図した比喩は残せます。"
        }
        SensoryRole::NonVisual
            if subject && predicate.object.is_some() && matches!(verb, "返す" | "反射") =>
        {
            "温度・匂い・味・触感が光を反射する主語になっています。光を返す物の表面と、そこから感じる性質を区別できるか確認してください。意図した比喩は残せます。"
        }
        SensoryRole::Silence if subject && matches!(verb, "鳴る" | "響く") => {
            "静けさが音を発する主語になっています。静けさの中で鳴った物を指すのか、逆説を意図しているのか確認してください。意図した比喩は残せます。"
        }
        SensoryRole::VisibleBody if object && matches!(verb, "聞く" | "聴く") => {
            "見える姿が聴覚動詞の目的語になっています。聞いた声や物音と、見えた姿の関係を確認してください。意図した比喩は残せます。"
        }
        // 「温度が上がる・伝わる・変わる」は値の変化として自然なので含めない。
        SensoryRole::Temperature
            if subject
                && matches!(
                    verb,
                    "移る"
                        | "溶ける"
                        | "暖める"
                        | "温める"
                        | "冷やす"
                        | "這う"
                        | "這い上がる"
                        | "漂う"
                        | "包む"
                        | "流れ込む"
                        | "染みる"
                        | "染み込む"
                        | "運ぶ"
                ) =>
        {
            "測った値としての温度が、移動したり物を温めたりする主語になっています。熱を運ぶ空気・地面・手などと、そこから感じる温度を区別できるか確認してください。熱の移る向きも前後と合うか確かめてください。意図した比喩は残せます。"
        }
        _ => return None,
    };
    Some((predicate.end, detail))
}

fn continued_predicate(tokens: &[&Morpheme], first: &Predicate) -> Option<usize> {
    if !matches!(
        tokens[first.head].normalized(),
        "響く" | "鳴る" | "流れ込む" | "広がる"
    ) || !tokens[first.end].pos(5).starts_with("連用形")
    {
        return None;
    }
    let mut next = first.end + 1;
    if tokens
        .get(next)
        .is_some_and(|t| t.pos(1) == "接続助詞" && matches!(t.surface.as_str(), "て" | "で"))
    {
        next += 1;
    } else if !tokens.get(next).is_some_and(|t| t.pos(1) == "読点") {
        return None;
    }
    if tokens.get(next).is_some_and(|t| t.pos(1) == "読点") {
        next += 1;
    }
    // 継続先では副詞だけを許す。新しい項や連体節を越えたゼロ主語を推定しない。
    while tokens.get(next).is_some_and(|t| t.pos(0) == "副詞") {
        next += 1;
        if tokens
            .get(next)
            .is_some_and(|t| t.surface == "に" && t.pos(0) == "助詞")
        {
            next += 1;
        }
    }
    let verb = tokens.get(next)?;
    (verb.pos(0) == "動詞" && matches!(verb.normalized(), "閉じる" | "開く" | "割れる" | "折れる"))
        .then_some(next)
}

fn auditory_modifier(tokens: &[&Morpheme], index: usize) -> Option<usize> {
    let verb = tokens[index];
    if verb.pos(0) != "動詞" || !matches!(verb.normalized(), "聞く" | "聴く" | "聞こえる")
    {
        return None;
    }
    let mut end = index;
    while tokens
        .get(end + 1)
        .is_some_and(|t| t.pos(0) == "助動詞" && t.dictionary_form() == "た")
    {
        end += 1;
    }
    if !tokens[end].pos(5).starts_with("連体形") {
        return None;
    }
    let mut head = None;
    for (next, token) in tokens.iter().enumerate().skip(end + 1).take(8) {
        match token.pos(0) {
            "名詞" => head = Some(next),
            "助詞" if token.surface == "の" => {}
            _ => break,
        }
    }
    head.filter(|&head| {
        matches!(
            sensory_argument(tokens, head),
            Some((SensoryRole::VisibleBody, _))
        )
    })
}

/// 名詞の直前にある連体節の動詞を返す。指示連体詞を一つだけ挟める。
/// 「時計の不在」のように別の名詞を修飾する節や、否定した動作は返さない。
fn attributive_verb(tokens: &[&Morpheme], noun: usize) -> Option<usize> {
    let mut index = noun.checked_sub(1)?;
    if tokens[index].pos(0) == "連体詞"
        && matches!(tokens[index].surface.as_str(), "その" | "この" | "あの")
    {
        index = index.checked_sub(1)?;
    }
    if !tokens[index].pos(5).starts_with("連体形") {
        return None;
    }
    loop {
        let token = tokens[index];
        if matches!(token.normalized(), "ない" | "無い" | "ぬ" | "ず" | "ん") {
            return None;
        }
        let auxiliary = token.pos(0) == "助動詞"
            || (token.pos(0) == "動詞" && token.pos(1) == "非自立可能")
            || (token.pos(1) == "接続助詞" && matches!(token.surface.as_str(), "て" | "で"));
        if !auxiliary {
            return (token.pos(0) == "動詞").then_some(index);
        }
        index = index.checked_sub(1)?;
    }
}

// nwiizo-coding-style: 連体節の動詞と修飾される名詞が隣接する2型だけを扱う;
// 修飾先が複数ありうる長い節や、名詞の後ろに続く述語は推定しない。
fn attributive_mismatch(tokens: &[&Morpheme], noun: usize) -> Option<(usize, &'static str)> {
    let head = tokens[noun];
    if head.pos(0) != "名詞" {
        return None;
    }
    let verb = attributive_verb(tokens, noun)?;
    let action = tokens[verb].normalized();
    match head.normalized() {
        "不在" | "欠如"
            if matches!(
                action,
                "唸る"
                    | "鳴る"
                    | "響く"
                    | "動く"
                    | "回る"
                    | "走る"
                    | "光る"
                    | "笑う"
                    | "話す"
                    | "喋る"
                    | "歩く"
                    | "吠える"
                    | "鳴く"
                    | "揺れる"
                    | "歌う"
            ) =>
        {
            Some((
                verb,
                "物や人の動作が「不在」を修飾しています。動いていた物と、それがなくなった状態を区別できるか確認してください。何がなくなったのかを推測で補わないでください。",
            ))
        }
        "光" | "明かり" | "音" | "声" | "響き" | "匂い" | "香り"
            if matches!(action, "被る" | "纏う" | "着る" | "羽織る") =>
        {
            Some((
                verb,
                "光・音・匂いが物を身にまとう形で修飾されています。埃や雪をかぶった物と、そこを通る光や音を区別できるか確認してください。意図した比喩は残せます。",
            ))
        }
        _ => None,
    }
}

pub(super) fn findings(tokenized: &[TokenizedSentence], raw_lines: &[&str]) -> Vec<Finding> {
    let mut findings = Vec::new();
    // 引用が複数の文・行に続く場合も、閉じ括弧まで候補から除く。
    let mut bracket_depth: usize = 0;
    for sentence in tokenized {
        let tokens = sentence
            .tokens
            .iter()
            .filter(|token| !token.surface.chars().all(|ch| matches!(ch, '*' | '_')))
            .collect::<Vec<_>>();
        for (index, token) in tokens.iter().enumerate() {
            match token.pos(1) {
                "括弧開" => bracket_depth += 1,
                "括弧閉" => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            if bracket_depth > 0 {
                continue;
            }
            if crate::markup::quotation_ranges(&sentence.text)
                .iter()
                .any(|range| range.contains(&token.byte_start))
            {
                continue;
            }
            if let Some(head) = auditory_modifier(&tokens, index) {
                findings.push(sentence.info_finding(raw_lines, token.byte_start..tokens[head].byte_end,
                    "sensory_role_mismatch", "聴覚の動作が、見える姿を修飾しています。聞こえたものと見えたものの関係を確認してください。共感覚を意図した表現は残せます。"));
            }
            if let Some((verb, detail)) = attributive_mismatch(&tokens, index) {
                findings.push(sentence.info_finding(
                    raw_lines,
                    tokens[verb].byte_start..token.byte_end,
                    "sensory_role_mismatch",
                    detail,
                ));
            }
            let Some((role, argument_end)) = sensory_argument(&tokens, index) else {
                continue;
            };
            let Some((predicate, detail)) = candidate_predicate(&tokens, argument_end, role) else {
                continue;
            };
            if !active_predicate(&tokens, predicate) {
                continue;
            }
            findings.push(sentence.info_finding(
                raw_lines,
                token.byte_start..tokens[predicate].byte_end,
                "sensory_role_mismatch",
                detail,
            ));
        }
    }
    findings
}
