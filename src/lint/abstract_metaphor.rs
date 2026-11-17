//! 語の族: 抽象的な対象を、地図・土台・羅針盤などの役割や「運ぶ」「効く」で表す言い回し
//! （`abstract_metaphor`）。語の出現だけでは判定せず、述語や格関係との結び付きで範囲を絞る。

use crate::morphology::Morpheme;

use super::Finding;
use super::morph::{TokenizedSentence, contains_any, punctuation_between, token_positions};

const ABSTRACT_METAPHOR_NOUNS: &[&str] = &[
    "地図",
    "羅針盤",
    "\u{5951}\u{7d04}",
    "道標",
    "土台",
    "架け橋",
];

const ABSTRACT_CONTEXT_NOUNS: &[&str] = &[
    "実装",
    "判断",
    "設計",
    "仕様",
    "方針",
    "計画",
    "戦略",
    "議論",
    "思考",
    "理解",
    "運用",
    "開発",
    "組織",
    "事業",
    "変革",
    "成長",
    "課題",
    "解決",
    "意思決定",
    "対話",
    "活用",
    "関係",
    "信頼",
    "経験",
    "価値",
];

/// 抽象的なものの「土台」「羅針盤」として使うと、何が何を支え、導くのかが読み取れなくなる名詞。
const FOUNDATION_NOUNS: &[&str] = &["土台", "羅針盤"];

/// 建物・機械の文脈。土台を実物の意味で使う（語の一致で見る。「家族」は含めない）。
const PHYSICAL_FOUNDATION_CONTEXT: &[&str] = &[
    "建物",
    "家",
    "住宅",
    "基礎",
    "コンクリート",
    "柱",
    "石垣",
    "機械",
    "工事",
    "建築",
];

/// 航海・登山の文脈。羅針盤を実物の意味で使う（「船具」も含めるよう文字列で見る）。
const NAVIGATION_CONTEXT: &[&str] = &["船", "航海", "方位", "磁石", "登山", "針路"];

/// 土台を育つものとして扱う動詞。
const GROWTH_VERBS: &[&str] = &["育つ", "育てる", "伸びる", "実る", "根付く", "芽生える"];

const ABSTRACT_TRANSPORT_SUBJECTS: &[&str] = &["仕様", "設計", "仕組み", "指標", "議論"];

const ABSTRACT_TRANSPORT_OBJECTS: &[&str] = &["意図", "判断", "理解", "実装", "成果"];

const ABSTRACT_EFFECT_SUBJECTS: &[&str] = &["複雑", "品質", "生産性", "リスク"];

const QUANTITY_NOUN_ENDINGS: &[&str] = &["量", "数"];

/// 抽象名詞を動作主にして移動や効果を表す、技術文書の翻訳調候補。
/// 物理的な運搬や一般的な「効く」を拾わないよう、観測済みの格関係と
/// 数量名詞の直後に限定する。
pub(super) fn abstract_predicate_metaphor_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        if let Some((start, end)) = abstract_transport_span(&sentence.tokens) {
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "abstract_metaphor",
                "抽象名詞と移動動詞の組み合わせ。意図や判断がどのように実装へ反映されるか、具体的に書けるか確認してください",
            ));
        }

        if let Some((start, end)) = abstract_effect_span(&sentence.tokens) {
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "abstract_metaphor",
                "抽象的な尺度と「で効く」の組み合わせ。何がどのように影響するか、具体的に書けるか確認してください",
            ));
        }
    }
    findings
}

fn abstract_transport_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let verb = tokens
        .iter()
        .position(|token| token.dictionary_form() == "運ぶ")?;
    let destination = tokens[..verb]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            ABSTRACT_TRANSPORT_OBJECTS
                .contains(&token.dictionary_form())
                .then_some(index)
                .filter(|index| {
                    tokens
                        .get(*index + 1)
                        .is_some_and(|particle| matches!(particle.surface.as_str(), "へ" | "に"))
                })
        })?;
    let object = tokens[..destination]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            ABSTRACT_TRANSPORT_OBJECTS
                .contains(&token.dictionary_form())
                .then_some(index)
                .filter(|index| {
                    tokens
                        .get(*index + 1)
                        .is_some_and(|particle| particle.surface == "を")
                })
        })?;
    let subject = tokens[..object]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            ABSTRACT_TRANSPORT_SUBJECTS
                .contains(&token.dictionary_form())
                .then_some(index)
                .filter(|index| {
                    tokens
                        .get(*index + 1)
                        .is_some_and(|particle| matches!(particle.surface.as_str(), "は" | "が"))
                })
        })?;
    if punctuation_between(tokens, subject, object)
        || punctuation_between(tokens, object, destination)
        || punctuation_between(tokens, destination, verb)
    {
        return None;
    }
    Some((tokens[subject].byte_start, tokens[verb].byte_end))
}

fn abstract_effect_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let verb = tokens
        .iter()
        .position(|token| token.dictionary_form() == "効く")?;
    let particle = verb.checked_sub(1)?;
    let quantity = particle.checked_sub(1)?;
    if tokens[particle].surface != "で"
        || tokens[quantity].pos(0) != "名詞"
        || !QUANTITY_NOUN_ENDINGS.iter().any(|ending| {
            tokens[quantity].dictionary_form() == *ending
                || tokens[quantity].surface.ends_with(ending)
        })
    {
        return None;
    }
    let (subject, subject_particle) = abstract_effect_subject(tokens, quantity)?;
    if subject_particle >= quantity
        || (subject_particle + 1..quantity).any(|index| {
            let token = &tokens[index];
            token.pos(0) == "助詞"
                && (token.surface == "が"
                    || (token.surface == "は" && tokens[index - 1].pos(0) != "助詞"))
        })
    {
        return None;
    }
    Some((tokens[subject].byte_start, tokens[verb].byte_end))
}

fn abstract_effect_subject(tokens: &[Morpheme], before: usize) -> Option<(usize, usize)> {
    tokens[..before]
        .iter()
        .enumerate()
        .find_map(|(index, token)| {
            if !ABSTRACT_EFFECT_SUBJECTS.contains(&token.dictionary_form()) {
                return None;
            }
            let suffix = tokens.get(index + 1);
            let particle_index = if token.dictionary_form() == "複雑"
                && suffix.is_some_and(|candidate| {
                    candidate.surface == "さ" && candidate.pos(0) == "接尾辞"
                }) {
                index + 2
            } else {
                index + 1
            };
            tokens.get(particle_index).and_then(|particle| {
                matches!(particle.surface.as_str(), "は" | "が").then_some((index, particle_index))
            })
        })
}

/// 抽象的な対象を物体になぞらえ、判断内容を曖昧にする可能性がある名詞句。
/// 候補語だけでは本来の意味での用例まで拾うため、述語として使われる場合か、
/// 抽象名詞から「の」で修飾される場合に限定する。
pub(super) fn abstract_metaphor_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
    include_technical_roles: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (sentence, index, token) in token_positions(tokenized) {
        let technical_role =
            include_technical_roles && matches!(token.dictionary_form(), "入口" | "主役");
        if token.pos(0) != "名詞"
            || (!ABSTRACT_METAPHOR_NOUNS.contains(&token.dictionary_form()) && !technical_role)
        {
            continue;
        }
        let tokens = &sentence.tokens;
        let foundation = FOUNDATION_NOUNS.contains(&token.dictionary_form());
        // 「土台作り」は準備の意味の定着語で、建物・機械・航海の文脈は実物を指す。
        if foundation
            && (tokens
                .get(index + 1)
                .is_some_and(|next| matches!(next.surface.as_str(), "作り" | "づくり" | "造り"))
                || contains_any(tokens, PHYSICAL_FOUNDATION_CONTEXT)
                || NAVIGATION_CONTEXT
                    .iter()
                    .any(|word| sentence.text.contains(word)))
        {
            continue;
        }

        let abstract_genitive = index >= 2
            && tokens[index - 1].surface == "の"
            && tokens[index - 2].pos(0) == "名詞"
            && is_abstract_context_noun(&tokens[index - 2]);
        let predicate_end = metaphor_predicate_end(tokens, index);
        // 土台・羅針盤は、文の前半の抽象語（読点の前も含む）が主題になる。羅針盤は述語にするだけで比喩になる。
        let context_before = if foundation {
            token.dictionary_form() == "羅針盤"
                || tokens[..index]
                    .iter()
                    .any(|token| token.pos(0) == "名詞" && is_abstract_context_noun(token))
        } else {
            has_abstract_context_before(tokens, index)
        };
        let extended = if foundation {
            based_on_span(tokens, index).or_else(|| growth_span(tokens, index))
        } else {
            None
        };
        let predicate = predicate_end.is_some() && context_before;
        if !abstract_genitive && !predicate && extended.is_none() {
            continue;
        }

        let byte_start = if index >= 2 && tokens[index - 1].surface == "の" {
            tokens[index - 2].byte_start
        } else {
            token.byte_start
        };
        let byte_end = predicate_end
            .map(|end| tokens[end].byte_end)
            .unwrap_or(token.byte_end);
        let (byte_start, byte_end) = if abstract_genitive || predicate {
            (byte_start, byte_end)
        } else {
            extended.unwrap_or((byte_start, byte_end))
        };
        findings.push(sentence.info_finding(
            raw_lines,
            byte_start..byte_end,
            "abstract_metaphor",
            format!(
                "抽象比喩の可能性: 「{}」。判断対象・判断基準・具体的な効果を明記してください",
                token.surface
            ),
        ));
    }
    findings
}

/// 「Xを土台に」で、Xが抽象語か、形容詞から作った性質の名詞（苦み・やさしさ）の範囲。
/// 「絵を土台に」のような実物は除く。
fn based_on_span(tokens: &[Morpheme], index: usize) -> Option<(usize, usize)> {
    let [object, particle] = tokens.get(index.checked_sub(2)?..index)? else {
        return None;
    };
    let quality = object.pos(1) == "普通名詞"
        && object.dictionary_form().chars().count() >= 2
        && object.dictionary_form().ends_with(['み', 'さ']);
    let next = tokens.get(index + 1)?;
    (particle.surface == "を"
        && object.pos(0) == "名詞"
        && (is_abstract_context_noun(object) || quality)
        && next.surface == "に")
        .then_some((object.byte_start, next.byte_end))
}

/// 「その土台は十分に育ちません」のように、土台を育つものとして扱う範囲。
fn growth_span(tokens: &[Morpheme], index: usize) -> Option<(usize, usize)> {
    let particle = tokens.get(index + 1)?;
    if particle.pos(0) != "助詞" || !matches!(particle.surface.as_str(), "は" | "が" | "も") {
        return None;
    }
    let verb = tokens[index + 2..]
        .iter()
        .take_while(|token| !matches!(token.surface.as_str(), "。" | "、"))
        .find(|token| token.pos(0) == "動詞" && GROWTH_VERBS.contains(&token.dictionary_form()))?;
    Some((tokens[index].byte_start, verb.byte_end))
}

fn is_abstract_context_noun(token: &Morpheme) -> bool {
    ABSTRACT_CONTEXT_NOUNS.iter().any(|candidate| {
        token.dictionary_form() == *candidate || token.surface.ends_with(candidate)
    })
}

fn has_abstract_context_before(tokens: &[Morpheme], noun_index: usize) -> bool {
    tokens[..noun_index]
        .iter()
        .rev()
        .take_while(|token| !matches!(token.pos(0), "記号" | "補助記号"))
        .take(12)
        .any(|token| token.pos(0) == "名詞" && is_abstract_context_noun(token))
}

fn metaphor_predicate_end(tokens: &[Morpheme], noun_index: usize) -> Option<usize> {
    let first = tokens.get(noun_index + 1)?;
    if matches!(first.dictionary_form(), "だ" | "です") {
        return Some(noun_index + 1);
    }

    let second = tokens.get(noun_index + 2)?;
    if matches!(first.surface.as_str(), "に" | "と") && second.dictionary_form() == "なる" {
        return Some(noun_index + 2);
    }
    if first.surface == "で" && second.dictionary_form() == "ある" {
        return Some(noun_index + 2);
    }
    if first.surface == "と" && second.dictionary_form() == "する" {
        let third = tokens.get(noun_index + 3)?;
        if third.surface == "て" {
            return Some(noun_index + 3);
        }
    }
    None
}
