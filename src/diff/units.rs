//! 比較の単位（文・箇条書き・見出し・引用行）の切り出しと、単位どうしの近さ。

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::Error;
use crate::lint::FORMAL_NOUNS;
use crate::morphology::{Morpheme, Morphology};
use crate::text::{
    heading, inline_code_ranges, is_list_item, mask_inline_spans,
    mask_markdown_structure_with_stats, protected_lines, sentences_with_raw, strip_line_prefix,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UnitKind {
    Prose,
    List,
    Heading,
    Quote,
}

pub(super) struct Unit {
    pub(super) line: usize,
    pub(super) kind: UnitKind,
    /// 見出しの階層。見出し以外は0。
    pub(super) level: usize,
    /// 文末の節の動詞が五段動詞から派生した可能動詞（読める、使える）か。
    pub(super) potential_tail: bool,
    pub(super) end_mark: Option<char>,
    /// コード・URLを空白にした本文。
    pub(super) text: String,
    pub(super) raw_text: String,
    /// 空白を除いた照合キー。
    pub(super) key: String,
    pub(super) tokens: Vec<Morpheme>,
    pub(super) bigrams: HashSet<(char, char)>,
    pub(super) lemmas: HashSet<String>,
    /// 明示的な「動詞＋ことができる」だけを規則的な可能形にそろえた本文と内容語。
    pub(super) ability_text: String,
    pub(super) ability_lemmas: HashSet<String>,
    pub(super) duplicate_commas: usize,
}

pub(super) fn units(raw: &str, morphology: &Morphology) -> Result<Vec<Unit>, Error> {
    let normalized = crate::markup::mask_review_emphasis(raw);
    let (masked, _) = mask_markdown_structure_with_stats(&normalized);
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let protected = protected_lines(&raw_lines);
    let mut lines = masked.split('\n').map(str::to_owned).collect::<Vec<_>>();
    let mut kinds = vec![UnitKind::Prose; raw_lines.len()];
    let mut levels = vec![0; raw_lines.len()];
    for (index, line) in raw_lines.iter().enumerate() {
        if protected[index] || !lines[index].trim().is_empty() {
            continue;
        }
        let kind = if is_list_item(line) {
            UnitKind::List
        } else if let Some((level, _)) = heading(line) {
            levels[index] = level;
            UnitKind::Heading
        } else if line.trim_start().starts_with('>') {
            UnitKind::Quote
        } else {
            continue;
        };
        kinds[index] = kind;
        lines[index] =
            crate::markup::mask_review_emphasis(&mask_inline_spans(&strip_line_prefix(line)));
    }
    let joined = lines.join("\n");
    // 引用を文や行に分割する前に除外する。分割後は閉じ括弧だけが残る場合がある。
    let duplicate_commas = crate::markup::duplicate_commas(&joined);
    let line_starts = lines
        .iter()
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some(start)
        })
        .collect::<Vec<_>>();
    let keys = sentences_with_raw(&joined, &normalized);
    sentences_with_raw(&joined, raw)
        .into_iter()
        .zip(keys)
        .map(|(sentence, normalized)| {
            let tokens = morphology.tokenize(&sentence.text)?;
            let ability_text = normalize_explicit_ability(&sentence.text, &tokens);
            let lemmas = content_lemmas(&tokens).into_iter().collect::<HashSet<_>>();
            let ability_lemmas = if ability_text == sentence.text {
                lemmas.clone()
            } else {
                content_lemmas(&morphology.tokenize(&ability_text)?)
                    .into_iter()
                    .collect()
            };
            // 照合キーはコード片やURLを含む原文から作る。本文の検査はマスク済みの text で行う。
            let code = inline_code_ranges(&normalized.raw_text);
            let key = normalized
                .raw_text
                .char_indices()
                .filter(|(index, ch)| {
                    !ch.is_whitespace() || code.iter().any(|span| span.contains(index))
                })
                .map(|(_, ch)| ch)
                .collect::<String>();
            Ok(Unit {
                duplicate_commas: duplicate_commas
                    .iter()
                    .filter(|range| {
                        let start = line_starts[sentence.line - 1] + sentence.line_byte_start;
                        range.start >= start && range.end <= start + sentence.text.len()
                    })
                    .count(),
                line: sentence.line,
                kind: kinds[sentence.line - 1],
                level: levels[sentence.line - 1],
                potential_tail: tail_potential(&tokens, &sentence.text, morphology),
                end_mark: sentence.end_mark,
                bigrams: bigrams(&key),
                lemmas,
                ability_text,
                ability_lemmas,
                key,
                tokens,
                text: sentence.text,
                raw_text: sentence.raw_text,
            })
        })
        .collect()
}

/// 元の文に可能の明示があるときだけ縮約する。「取れる」単独の語義は決めない。
/// 使役・不規則活用は対象外。語幹はlintの候補生成と共用し、後続の否定・時制を残す。
fn normalize_explicit_ability(text: &str, tokens: &[Morpheme]) -> String {
    let mut normalized = text.to_owned();
    for phrase in tokens.windows(4).rev() {
        let (verb, noun, particle, ability) = (&phrase[0], &phrase[1], &phrase[2], &phrase[3]);
        if noun.surface != "こと"
            || noun.pos(0) != "名詞"
            || particle.surface != "が"
            || particle.pos(0) != "助詞"
            || ability.pos(0) != "動詞"
            || !ability.surface.starts_with("でき")
            || !phrase
                .windows(2)
                .all(|pair| pair[0].byte_end == pair[1].byte_start)
        {
            continue;
        }
        if let Some(stem) = verb.potential_stem() {
            normalized.replace_range(
                verb.byte_start..ability.byte_end,
                &format!("{stem}{}", &ability.surface["でき".len()..]),
            );
        }
    }
    normalized
}

// 五段動詞の可能動詞と同じ形を持つ、別の意味の動詞（他動詞・自動詞の対など）。
// 「入れる」は「入る」の可能動詞ではなく、「進める」は「進む」の可能動詞ではない。
const NON_POTENTIAL_VERBS: &[&str] = &[
    "入れる",
    "切れる",
    "売れる",
    "取れる",
    "割れる",
    "折れる",
    "破れる",
    "知れる",
    "焼ける",
    "解ける",
    "溶ける",
    "抜ける",
    "欠ける",
    "裂ける",
    "剥ける",
    "砕ける",
    "脱げる",
    "開ける",
    "空ける",
    "明ける",
    "付ける",
    "着ける",
    "届ける",
    "傾ける",
    "続ける",
    "向ける",
    "設ける",
    "片付ける",
    "近づける",
    "傷つける",
    "退ける",
    "避ける",
    "助ける",
    "受ける",
    "掛ける",
    "懸ける",
    "賭ける",
    "預ける",
    "授ける",
    "儲ける",
    "分ける",
    "建てる",
    "立てる",
    "育てる",
    "当てる",
    "進める",
    "止める",
    "休める",
    "緩める",
    "痛める",
    "温める",
    "弱める",
    "強める",
    "深める",
    "固める",
    "高める",
    "広める",
    "狭める",
    "早める",
    "速める",
    "薄める",
    "丸める",
    "含める",
    "苦しめる",
    "埋める",
    "詰める",
    "締める",
    "占める",
    "攻める",
    "責める",
    "納める",
    "収める",
    "治める",
    "修める",
    "勧める",
    "薦める",
    "眺める",
    "染める",
    "褒める",
    "鎮める",
    "沈める",
    "清める",
    "秘める",
    "泊める",
    "集める",
    "決める",
    "始める",
    "定める",
    "改める",
    "認める",
    "求める",
    "務める",
    "勤める",
    "努める",
    "伝える",
    "従える",
    "備える",
    "加える",
    "迎える",
    "与える",
    "考える",
    "教える",
    "覚える",
    "抑える",
    "構える",
    "捉える",
    "蓄える",
    "唱える",
    "訴える",
    "支える",
    "変える",
    "替える",
    "植える",
    "添える",
    "据える",
    "控える",
    "整える",
    "揃える",
    "答える",
    "応える",
    "耐える",
    "絶える",
    "数える",
    "仕える",
    "栄える",
    "消える",
    "見える",
    "増える",
    "冷える",
    "燃える",
    "生える",
    "越える",
    "超える",
    "乗せる",
    "載せる",
    "寄せる",
    "見せる",
    "任せる",
    "伏せる",
    "着せる",
    "合わせる",
    "知らせる",
    "並べる",
    "比べる",
    "調べる",
    "食べる",
    "述べる",
    "浮かべる",
    "上げる",
    "下げる",
    "曲げる",
    "告げる",
    "妨げる",
    "広げる",
    "繋げる",
    "投げる",
    "逃げる",
    "挙げる",
    "揚げる",
    "掲げる",
    "和らげる",
    "遂げる",
    "仕上げる",
    "取り上げる",
    "流れる",
    "離れる",
    "外れる",
    "疲れる",
    "遅れる",
    "恐れる",
    "忘れる",
    "壊れる",
    "触れる",
    "濡れる",
    "乱れる",
    "崩れる",
    "隠れる",
    "暴れる",
    "揺れる",
    "溢れる",
    "優れる",
    "現れる",
    "表れる",
    "生まれる",
    "恵まれる",
    "慣れる",
    "枯れる",
    "垂れる",
    "晴れる",
    "腫れる",
    "擦れる",
    "群れる",
    "逸れる",
    "汚れる",
    "倒れる",
];

/// 辞書形が「読める」「使える」のように五段動詞のエ段＋「る」で、エ段をウ段に戻した語が
/// 辞書で五段動詞として引けるとき、可能動詞と見る。対になる他動詞・自動詞は一覧で除く。
fn is_potential_verb(dictionary_form: &str, morphology: &Morphology) -> bool {
    if NON_POTENTIAL_VERBS.contains(&dictionary_form) {
        return false;
    }
    let mut chars = dictionary_form.chars().collect::<Vec<_>>();
    if chars.len() < 3 || chars.pop() != Some('る') {
        return false;
    }
    let Some(e_row) = chars.pop() else {
        return false;
    };
    let u_row = match e_row {
        'え' => 'う',
        'け' => 'く',
        'げ' => 'ぐ',
        'せ' => 'す',
        'て' => 'つ',
        'ね' => 'ぬ',
        'べ' => 'ぶ',
        'め' => 'む',
        'れ' => 'る',
        _ => return false,
    };
    chars.push(u_row);
    let base = chars.into_iter().collect::<String>();
    morphology.tokenize(&base).is_ok_and(|tokens| {
        tokens.len() == 1
            && tokens[0].pos(0) == "動詞"
            && tokens[0].pos(4).starts_with("五段")
            && tokens[0].dictionary_form() == base
    })
}

pub(super) fn tail_start(text: &str) -> usize {
    text.rfind('、').map_or(0, |index| index + '、'.len_utf8())
}

/// 文末の節の最後の動詞が可能動詞か。
fn tail_potential(tokens: &[Morpheme], text: &str, morphology: &Morphology) -> bool {
    let start = tail_start(text);
    tokens
        .iter()
        .rev()
        .find(|token| {
            token.byte_start >= start && token.pos(0) == "動詞" && token.pos(1) != "非自立可能"
        })
        .is_some_and(|verb| is_potential_verb(verb.dictionary_form(), morphology))
}

pub(super) fn bigrams(key: &str) -> HashSet<(char, char)> {
    let chars = key.chars().collect::<Vec<_>>();
    chars.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

pub(super) fn similarity(left: &Unit, right: &Unit) -> f64 {
    if left.key == right.key {
        return 1.0;
    }
    let total = left.bigrams.len() + right.bigrams.len();
    if total == 0 {
        return 0.0;
    }
    let shared = left.bigrams.intersection(&right.bigrams).count();
    2.0 * shared as f64 / total as f64
}

pub(super) const LIGHT_VERBS: &[&str] = &[
    "する",
    "為る",
    "知れる",
    "しれる",
    "ある",
    "有る",
    "いる",
    "居る",
    "なる",
    "成る",
    "できる",
    "出来る",
    "行う",
    "いう",
    "言う",
    "持つ",
    "おく",
    "置く",
    "くる",
    "来る",
    "いく",
    "行く",
    "みる",
    "見る",
    "しまう",
    "くれる",
    "もらう",
    "くださる",
    "いただく",
    "得る",
];

pub(super) fn content_lemmas(tokens: &[Morpheme]) -> Vec<String> {
    tokens
        .iter()
        .filter(|token| match token.pos(0) {
            "名詞" => {
                matches!(token.pos(1), "普通名詞" | "固有名詞")
                    && !FORMAL_NOUNS.contains(&token.surface.as_str())
            }
            "動詞" => {
                token.pos(1) != "非自立可能" && !LIGHT_VERBS.contains(&token.dictionary_form())
            }
            "形容詞" => token.pos(1) != "非自立可能",
            "形状詞" => true,
            _ => false,
        })
        .map(|token| token.normalized().to_owned())
        .collect()
}

pub(super) static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s）)」>]+").expect("valid URL regex"));
