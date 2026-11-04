//! 改稿前後の文書を文単位で対応づけ、意味が変わった可能性のある箇所を候補として返す。
//! 数値・否定・文末の働き・条件・識別子・引用・主語の増減を、形態素と表層で照合する。
//! 意味が同じかどうかは判定しない。候補が出ない変更も読み直す対象である。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::Error;
use crate::lint::FORMAL_NOUNS;
use crate::morphology::{Morpheme, Morphology};
use crate::text::{
    fenced_lines, heading, inline_code_ranges, is_list_item, mask_inline_spans,
    mask_markdown_structure_with_stats, protected_lines, sentences_with_raw, strip_line_prefix,
};

/// 入力本文のSHA-256（16進小文字）。検査した本文と最終稿が同じかを照らすために返す。
pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffFinding {
    pub category: String,
    pub severity: String,
    /// 変更前の対象行。変更後だけにある箇所はNone。
    pub before_line: Option<usize>,
    /// 変更後の対象行。変更前だけにある箇所はNone。
    pub after_line: Option<usize>,
    pub before_excerpt: String,
    pub after_excerpt: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffStats {
    pub total_findings: usize,
    pub by_category: BTreeMap<String, usize>,
    /// 文書を文・箇条書き・見出し・引用行に分けた単位の数。
    pub before_units: usize,
    pub after_units: usize,
    /// 空白を除いて一致した単位の数。
    pub matched_units: usize,
    /// 位置だけが変わった単位の数（前後の合計）。
    pub moved_units: usize,
    /// 一致しない単位が連続する領域の数。
    pub changed_regions: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffReport {
    pub stats: DiffStats,
    pub findings: Vec<DiffFinding>,
}

const EXCERPT_CHARS: usize = 80;
const PAIR_SIMILARITY: f64 = 0.3;
const NEGATION_SIMILARITY: f64 = 0.6;
const COVERAGE_KEPT: f64 = 0.5;
const LCS_CELLS: usize = 40_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnitKind {
    Prose,
    List,
    Heading,
    Quote,
}

struct Unit {
    line: usize,
    kind: UnitKind,
    /// 見出しの階層。見出し以外は0。
    level: usize,
    /// 文末の節の動詞が五段動詞から派生した可能動詞（読める、使える）か。
    potential_tail: bool,
    end_mark: Option<char>,
    /// コード・URLを空白にした本文。
    text: String,
    raw_text: String,
    /// 空白を除いた照合キー。
    key: String,
    tokens: Vec<Morpheme>,
    bigrams: HashSet<(char, char)>,
    lemmas: HashSet<String>,
    /// 明示的な「動詞＋ことができる」だけを規則的な可能形にそろえた本文と内容語。
    ability_text: String,
    ability_lemmas: HashSet<String>,
    duplicate_commas: usize,
}

pub fn compare(before: &str, after: &str, morphology: &Morphology) -> Result<DiffReport, Error> {
    let before_units = units(before, morphology)?;
    let after_units = units(after, morphology)?;
    let pairs = align(&before_units, &after_units);
    let (regions, moved) = regions(&before_units, &after_units, &pairs);
    // 文書全体にある値は、別の文へ移しただけでも増減として数えない。
    let document_numbers = DocumentNumbers {
        before: numbers_in(&before_units),
        after: numbers_in(&after_units),
    };

    let mut findings = Vec::new();
    for region in &regions {
        let before_block = region
            .before
            .iter()
            .map(|index| &before_units[*index])
            .collect::<Vec<_>>();
        let after_block = region
            .after
            .iter()
            .map(|index| &after_units[*index])
            .collect::<Vec<_>>();
        analyze_region(
            &before_block,
            &after_block,
            &document_numbers,
            &mut findings,
        );
    }
    findings.extend(protected_findings(before, after));
    let document_register = register_findings(&before_units, &after_units);
    // 文書全体の文体を変えた場合は、文ごとの切り替えを重ねて示さない。
    if !document_register.is_empty() {
        findings.retain(|finding| finding.category != "sentence_register_changed");
    }
    findings.extend(document_register);
    findings.extend(tone_findings(&before_units, &after_units));
    findings.extend(addressee_findings(before, after));
    findings.sort_by_key(|finding| {
        (
            finding.before_line.or(finding.after_line),
            finding.after_line,
            finding.category.clone(),
        )
    });

    let mut by_category = BTreeMap::new();
    for finding in &findings {
        *by_category.entry(finding.category.clone()).or_default() += 1;
    }
    Ok(DiffReport {
        stats: DiffStats {
            total_findings: findings.len(),
            by_category,
            before_units: before_units.len(),
            after_units: after_units.len(),
            matched_units: pairs.len(),
            moved_units: moved,
            changed_regions: regions.len(),
        },
        findings,
    })
}

// ---- 単位の切り出し ----

fn units(raw: &str, morphology: &Morphology) -> Result<Vec<Unit>, Error> {
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

fn tail_start(text: &str) -> usize {
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

fn bigrams(key: &str) -> HashSet<(char, char)> {
    let chars = key.chars().collect::<Vec<_>>();
    chars.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

fn similarity(left: &Unit, right: &Unit) -> f64 {
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

// ---- 対応づけ ----

/// 空白を除いて一致する単位を、文書内で一度しか現れない単位を錨にして前から対応づける。
/// 錨のない範囲は小さければ最長共通部分列で、大きければ両端からの一致だけで対応づける。
fn align(before: &[Unit], after: &[Unit]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    align_range(before, after, 0..before.len(), 0..after.len(), &mut pairs);
    pairs.sort_unstable();
    pairs
}

fn align_range(
    before: &[Unit],
    after: &[Unit],
    b: Range<usize>,
    a: Range<usize>,
    pairs: &mut Vec<(usize, usize)>,
) {
    let (mut bs, mut be, mut as_, mut ae) = (b.start, b.end, a.start, a.end);
    while bs < be && as_ < ae && before[bs].key == after[as_].key {
        pairs.push((bs, as_));
        bs += 1;
        as_ += 1;
    }
    while bs < be && as_ < ae && before[be - 1].key == after[ae - 1].key {
        pairs.push((be - 1, ae - 1));
        be -= 1;
        ae -= 1;
    }
    if bs >= be || as_ >= ae {
        return;
    }
    let anchors = unique_anchors(before, after, bs..be, as_..ae);
    if anchors.is_empty() {
        if (be - bs) * (ae - as_) <= LCS_CELLS {
            lcs_pairs(before, after, bs..be, as_..ae, pairs);
        }
        return;
    }
    let mut previous = (bs, as_);
    for (bi, aj) in anchors {
        pairs.push((bi, aj));
        align_range(before, after, previous.0..bi, previous.1..aj, pairs);
        previous = (bi + 1, aj + 1);
    }
    align_range(before, after, previous.0..be, previous.1..ae, pairs);
}

/// 範囲内で前後ともに一度だけ現れるキーを、位置が単調に増える最長の列に絞る。
fn unique_anchors(
    before: &[Unit],
    after: &[Unit],
    b: Range<usize>,
    a: Range<usize>,
) -> Vec<(usize, usize)> {
    let mut before_positions = HashMap::<&str, Vec<usize>>::new();
    for index in b {
        before_positions
            .entry(before[index].key.as_str())
            .or_default()
            .push(index);
    }
    let mut after_positions = HashMap::<&str, Vec<usize>>::new();
    for index in a {
        after_positions
            .entry(after[index].key.as_str())
            .or_default()
            .push(index);
    }
    let mut candidates = before_positions
        .iter()
        .filter(|(_, positions)| positions.len() == 1)
        .filter_map(|(key, positions)| {
            after_positions
                .get(key)
                .filter(|after| after.len() == 1)
                .map(|after| (positions[0], after[0]))
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    longest_increasing(&candidates)
}

fn longest_increasing(candidates: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails = Vec::<usize>::new();
    let mut previous = vec![usize::MAX; candidates.len()];
    for (index, (_, aj)) in candidates.iter().enumerate() {
        let position = tails.partition_point(|&tail| candidates[tail].1 < *aj);
        if position > 0 {
            previous[index] = tails[position - 1];
        }
        if position == tails.len() {
            tails.push(index);
        } else {
            tails[position] = index;
        }
    }
    let mut chain = Vec::new();
    let mut current = tails.last().copied();
    while let Some(index) = current {
        chain.push(candidates[index]);
        current = (previous[index] != usize::MAX).then(|| previous[index]);
    }
    chain.reverse();
    chain
}

fn lcs_pairs(
    before: &[Unit],
    after: &[Unit],
    b: Range<usize>,
    a: Range<usize>,
    pairs: &mut Vec<(usize, usize)>,
) {
    let (rows, cols) = (b.len(), a.len());
    let mut table = vec![vec![0u32; cols + 1]; rows + 1];
    for i in (0..rows).rev() {
        for j in (0..cols).rev() {
            table[i][j] = if before[b.start + i].key == after[a.start + j].key {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < rows && j < cols {
        if before[b.start + i].key == after[a.start + j].key {
            pairs.push((b.start + i, a.start + j));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
}

struct Region {
    before: Vec<usize>,
    after: Vec<usize>,
}

/// 一致しない単位を領域にまとめる。前後の両方で一致しないまま残った同じキーは、
/// 位置が変わっただけの単位として領域から外す。
fn regions(before: &[Unit], after: &[Unit], pairs: &[(usize, usize)]) -> (Vec<Region>, usize) {
    let matched_before = pairs.iter().map(|(b, _)| *b).collect::<HashSet<_>>();
    let matched_after = pairs.iter().map(|(_, a)| *a).collect::<HashSet<_>>();
    let mut unmatched_after = HashMap::<&str, usize>::new();
    for (index, unit) in after.iter().enumerate() {
        if !matched_after.contains(&index) {
            *unmatched_after.entry(unit.key.as_str()).or_default() += 1;
        }
    }
    let mut moved_before = HashSet::new();
    let mut moved_after_keys = HashMap::<&str, usize>::new();
    for (index, unit) in before.iter().enumerate() {
        if matched_before.contains(&index) {
            continue;
        }
        if let Some(count) = unmatched_after.get_mut(unit.key.as_str())
            && *count > 0
        {
            *count -= 1;
            moved_before.insert(index);
            *moved_after_keys.entry(unit.key.as_str()).or_default() += 1;
        }
    }
    let mut moved_after = HashSet::new();
    for (index, unit) in after.iter().enumerate() {
        if matched_after.contains(&index) {
            continue;
        }
        if let Some(count) = moved_after_keys.get_mut(unit.key.as_str())
            && *count > 0
        {
            *count -= 1;
            moved_after.insert(index);
        }
    }

    let mut regions = Vec::new();
    let mut cursor = (0, 0);
    let mut boundaries = pairs.to_vec();
    boundaries.push((before.len(), after.len()));
    for (bi, aj) in boundaries {
        let region = Region {
            before: (cursor.0..bi)
                .filter(|index| !moved_before.contains(index))
                .collect(),
            after: (cursor.1..aj)
                .filter(|index| !moved_after.contains(index))
                .collect(),
        };
        if !region.before.is_empty() || !region.after.is_empty() {
            regions.push(region);
        }
        cursor = (bi + 1, aj + 1);
    }
    (regions, moved_before.len() + moved_after.len())
}

// ---- 領域ごとの照合 ----

fn excerpt(units: &[&Unit]) -> String {
    let joined = units
        .iter()
        .map(|unit| unit.raw_text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut excerpt = joined.chars().take(EXCERPT_CHARS).collect::<String>();
    if joined.chars().count() > EXCERPT_CHARS {
        excerpt.push('…');
    }
    excerpt
}

fn finding(
    category: &str,
    severity: &str,
    before: &[&Unit],
    after: &[&Unit],
    detail: String,
) -> DiffFinding {
    DiffFinding {
        category: category.to_owned(),
        severity: severity.to_owned(),
        before_line: before.first().map(|unit| unit.line),
        after_line: after.first().map(|unit| unit.line),
        before_excerpt: excerpt(before),
        after_excerpt: excerpt(after),
        detail,
    }
}

fn block_text(units: &[&Unit]) -> String {
    units
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn block_raw(units: &[&Unit]) -> String {
    units
        .iter()
        .map(|unit| unit.raw_text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 文書全体に現れる数値の種類。
struct DocumentNumbers {
    before: BTreeSet<String>,
    after: BTreeSet<String>,
}

fn numbers_in(units: &[Unit]) -> BTreeSet<String> {
    units.iter().flat_map(|unit| numbers(&unit.text)).collect()
}

fn analyze_region(
    before: &[&Unit],
    after: &[&Unit],
    document_numbers: &DocumentNumbers,
    findings: &mut Vec<DiffFinding>,
) {
    if before.is_empty() {
        findings.push(finding(
            "sentence_added",
            "warn",
            before,
            after,
            format!(
                "変更前にない文が{}件あります。原文や資料に根拠がある内容か、新しい主体・原因・効果を書き足していないか確認してください。",
                after.len()
            ),
        ));
        return;
    }
    if after.is_empty() {
        findings.push(finding(
            "sentence_removed",
            "warn",
            before,
            after,
            format!(
                "変更後にない文が{}件あります。条件・例外・数値・理由が一緒に消えていないか確認してください。",
                before.len()
            ),
        ));
        return;
    }
    let claimed_by = pair_units(before, after);
    // 対応する文がなく、内容語も相手側に残っていない文は削除・追加として示し、
    // 残りの文（書き換えと、分割・結合で形を変えた文）だけを語句の照合にかける。
    let mut kept_before = Vec::new();
    for (b_index, b_unit) in before.iter().enumerate() {
        if claimed_by.contains(&Some(b_index)) || coverage(b_unit, after) >= COVERAGE_KEPT {
            kept_before.push(*b_unit);
        } else {
            findings.push(finding(
                "sentence_removed",
                "warn",
                &[b_unit],
                &[],
                "変更後に対応する文がありません。条件・例外・数値・理由が一緒に消えていないか確認してください。".to_owned(),
            ));
        }
    }
    let mut kept_after = Vec::new();
    for (a_index, a_unit) in after.iter().enumerate() {
        if claimed_by[a_index].is_some() || coverage(a_unit, before) >= COVERAGE_KEPT {
            kept_after.push(*a_unit);
        } else {
            findings.push(finding(
                "sentence_added",
                "warn",
                &[],
                &[a_unit],
                "変更前に対応する文がありません。原文や資料に根拠がある内容か、新しい主体・原因・効果を書き足していないか確認してください。".to_owned(),
            ));
        }
    }
    pair_findings(before, after, &claimed_by, findings);
    if kept_before.is_empty() || kept_after.is_empty() {
        return;
    }
    feeling_findings(&kept_before, &kept_after, findings);
    promise_findings(&kept_before, &kept_after, findings);
    number_findings(&kept_before, &kept_after, document_numbers, findings);
    certainty_findings(&kept_before, &kept_after, findings);
    condition_findings(&kept_before, &kept_after, findings);
    identifier_findings(&kept_before, &kept_after, findings);
    quotation_findings(&kept_before, &kept_after, findings);
    citation_findings(&kept_before, &kept_after, findings);
    subject_added_findings(&kept_before, &kept_after, findings);
    recipient_added_findings(&kept_before, &kept_after, findings);
    nominalization_added_findings(&kept_before, &kept_after, findings);
    content_word_findings(&kept_before, &kept_after, findings);
    actor_narrowed_findings(&kept_before, &kept_after, findings);
}

/// 変更後の各文に、最も似た変更前の文を割り当てる。1文が1文に置き換わった領域は無条件に対にする。
fn pair_units(before: &[&Unit], after: &[&Unit]) -> Vec<Option<usize>> {
    let mut claimed_by = vec![None::<usize>; after.len()];
    if before.len() == 1 && after.len() == 1 {
        claimed_by[0] = Some(0);
        return claimed_by;
    }
    for (a_index, a_unit) in after.iter().enumerate() {
        let best = before
            .iter()
            .enumerate()
            .map(|(b_index, b_unit)| (b_index, pair_score(b_unit, a_unit)))
            .filter(|(_, score)| *score >= PAIR_SIMILARITY)
            .max_by(|left, right| left.1.total_cmp(&right.1).then(right.0.cmp(&left.0)));
        claimed_by[a_index] = best.map(|(index, _)| index);
    }
    claimed_by
}

fn multiset_diff(before: &[String], after: &[String]) -> (Vec<String>, Vec<String>) {
    let mut counts = BTreeMap::<&str, i64>::new();
    for item in before {
        *counts.entry(item.as_str()).or_default() += 1;
    }
    for item in after {
        *counts.entry(item.as_str()).or_default() -= 1;
    }
    let mut lost = Vec::new();
    let mut added = Vec::new();
    for (item, count) in counts {
        for _ in 0..count.max(0) {
            lost.push(item.to_owned());
        }
        for _ in 0..(-count).max(0) {
            added.push(item.to_owned());
        }
    }
    (lost, added)
}

fn join_items(items: &[String]) -> String {
    const LIMIT: usize = 8;
    let mut text = items
        .iter()
        .take(LIMIT)
        .map(|item| format!("「{item}」"))
        .collect::<Vec<_>>()
        .join("、");
    if items.len() > LIMIT {
        text.push_str(&format!("ほか{}件", items.len() - LIMIT));
    }
    if text.is_empty() {
        text.push_str("なし");
    }
    text
}

// 数値。桁区切りと全角・半角の違いは同じ値として扱い、約・最大・以上などの修飾を含めて比べる。
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:約|およそ|おおよそ|最大|最小|最低|最高|最長|最短|少なくとも|せいぜい|合計|延べ)?",
        r"[-−－+＋]?[0-9０-９][0-9０-９,，]*(?:[.．][0-9０-９]+)*",
        r"(?:[万億兆])?",
        r"(?:[ \t]*(?:[%％‰]|[A-Za-z]{1,4}|週間|時間|日間|年間|か月|カ月|ヶ月|ヵ月|回目|件目|番目|文字|ページ|か所|カ所|ヶ所|箇所|段階|種類|世代|件|回|人|名|日|年|月|週|分|秒|倍|割|円|個|つ|本|枚|台|行|桁|字|語|頁|章|節|点|社|番|位|歳|度|期))?",
        r"(?:以上|以下|未満|以内|前後|程度|ほど|弱|強|超|以降|以前)?",
    ))
    .expect("valid number regex")
});
static KANJI_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"[一二三四五六七八九十百千]+(?:つ|回|件|人|名|日|年|か月|カ月|ヶ月|週間|時間|秒|倍|割|番目|段階|種類|点|社|本|枚|行|文字|字|個|台)",
    )
    .expect("valid kanji number regex")
});

fn normalize_number(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, ',' | '，' | ' ' | '\t'))
        .map(|ch| match ch {
            '０'..='９' => char::from_u32(ch as u32 - '０' as u32 + '0' as u32).unwrap_or(ch),
            '．' => '.',
            '％' => '%',
            '−' | '－' => '-',
            '＋' => '+',
            other => other,
        })
        .collect()
}

pub(crate) fn numbers(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for found_match in NUMBER.find_iter(text) {
        let head = &text[..found_match.start()];
        let preceded_by_ascii = head
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '.');
        // 脚注・引用番号の [1] や [^2] は引用の印として別に比べる。
        let bracketed = (head.ends_with('[') || head.ends_with("[^"))
            && text[found_match.end()..].starts_with(']');
        if !preceded_by_ascii && !bracketed {
            found.push(normalize_number(found_match.as_str()));
        }
    }
    found.extend(
        KANJI_NUMBER
            .find_iter(text)
            .map(|found_match| found_match.as_str().to_owned()),
    );
    found
}

/// 消えた項目か増えた項目があれば、その一覧を説明に入れた候補を1件作る。
fn item_change(
    category: &str,
    severity: &str,
    before: &[&Unit],
    after: &[&Unit],
    lost: &[String],
    added: &[String],
    describe: impl FnOnce(String, String) -> String,
) -> Option<DiffFinding> {
    (!lost.is_empty() || !added.is_empty()).then(|| {
        finding(
            category,
            severity,
            before,
            after,
            describe(join_items(lost), join_items(added)),
        )
    })
}

/// 同じ値の繰り返しは数えず、値の種類だけを比べる。
fn distinct(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn number_findings(
    before: &[&Unit],
    after: &[&Unit],
    document_numbers: &DocumentNumbers,
    findings: &mut Vec<DiffFinding>,
) {
    let (lost, added) = multiset_diff(
        &distinct(numbers(&block_text(before))),
        &distinct(numbers(&block_text(after))),
    );
    let lost = lost
        .into_iter()
        .filter(|value| !document_numbers.after.contains(value))
        .collect::<Vec<_>>();
    let added = added
        .into_iter()
        .filter(|value| !document_numbers.before.contains(value))
        .collect::<Vec<_>>();
    findings.extend(item_change("number_changed", "warn", before, after, &lost, &added, |lost, added| {
        format!(
            "数値や単位の表現が変わりました。消えた値: {lost} / 増えた値: {added}。約・最大・以上などの修飾を含めて、値と範囲が原文と同じか確認してください。"
        )
    }));
}

// 否定。義務・推量・限定などの慣用句は否定の数に入れない。
static NEGATION_IDIOMS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"かもしれ(?:ない|ません|ぬ)|に違い(?:ない|ありません)|とは限(?:らない|りません)|わけでは(?:ない|ありません)|",
        r"に(?:すぎ|過ぎ)(?:ない|ません)|しか(?:ない|ありません)|ほか(?:ない|ありません)|",
        r"なければ(?:ならない|なりません|いけない|いけません|ならず)|なくては(?:ならない|なりません|いけない|いけません)|",
        r"ないと(?:いけない|いけません|ならない|なりません)|ざるを得(?:ない|ません)|ねばなら(?:ない|ぬ|ず)|",
        r"(?:では|じゃ)な(?:く|くて)|だけでなく|のみならず|問題(?:は|が|も)?(?:ない|なく|ありません)|",
        r"差し支え(?:ない|ありません)|構(?:わない|いません)|かまいません|仕方(?:が|の)?(?:ない|ありません)|",
        r"少なく(?:ない|ありません)|なくない|有無|否か",
    ))
    .expect("valid negation idiom regex")
});
static NEGATION_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"不(?:要|可|明|正|在|備|適|十分|一致|安定|許可|採用|使用|参加|可能|完全)|",
        r"未[一-龥]|非(?:公開|対応|表示|推奨|同期|互換|同意|活性)|無(?:効|料|視|関係|制限|断)|禁止|厳禁",
    ))
    .expect("valid negation prefix regex")
});

fn negation_marks(unit: &Unit) -> Vec<String> {
    let idioms = NEGATION_IDIOMS
        .find_iter(&unit.text)
        .map(|found| found.range())
        .collect::<Vec<_>>();
    let inside_idiom = |byte: usize| idioms.iter().any(|range| range.contains(&byte));
    let mut marks = Vec::new();
    for (index, token) in unit.tokens.iter().enumerate() {
        if inside_idiom(token.byte_start) {
            continue;
        }
        let negative = match token.pos(0) {
            "助動詞" => matches!(token.dictionary_form(), "ない" | "ぬ" | "ず"),
            "形容詞" => matches!(token.dictionary_form(), "ない" | "無い"),
            _ => false,
        };
        if negative {
            // 「ません」は「ませ」＋「ん」に分かれるので、表示は語のまとまりに戻す。
            let previous = index
                .checked_sub(1)
                .map(|index| unit.tokens[index].surface.as_str());
            marks.push(match (previous, token.surface.as_str()) {
                (Some("ませ"), "ん") => "ません".to_owned(),
                _ => token.surface.clone(),
            });
        }
    }
    for found in NEGATION_PREFIX.find_iter(&unit.text) {
        if inside_idiom(found.start()) || matches!(found.as_str(), "未来" | "未明" | "未満") {
            continue;
        }
        marks.push(found.as_str().to_owned());
    }
    marks
}

/// 文末の節（最後の読点より後ろ）に否定の語があるか。
fn tail_negated(unit: &Unit) -> bool {
    let tail_start = unit
        .text
        .rfind('、')
        .map_or(0, |index| index + '、'.len_utf8());
    let idioms = NEGATION_IDIOMS
        .find_iter(&unit.text)
        .map(|found| found.range())
        .collect::<Vec<_>>();
    unit.tokens.iter().any(|token| {
        token.byte_start >= tail_start
            && !idioms.iter().any(|range| range.contains(&token.byte_start))
            && match token.pos(0) {
                "助動詞" => matches!(token.dictionary_form(), "ない" | "ぬ" | "ず"),
                "形容詞" => matches!(token.dictionary_form(), "ない" | "無い"),
                _ => false,
            }
    }) || NEGATION_PREFIX
        .find_iter(&unit.text[tail_start..])
        .any(|found| !matches!(found.as_str(), "未来" | "未明" | "未満"))
}

/// 対になった文で否定の数が違い、字面がほぼ同じ（否定だけを変えた）か文末の肯定・否定が
/// 入れ替わった場合に示す。文全体を書き直して否定の数が変わっただけの文は、内容語の変化として読む。
fn negation_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let before_marks = negation_marks(before);
    let after_marks = negation_marks(after);
    if before_marks.len() == after_marks.len()
        || (similarity(before, after) < NEGATION_SIMILARITY
            && tail_negated(before) == tail_negated(after))
    {
        return;
    }
    findings.push(finding(
        "negation_changed",
        "warn",
        &[before],
        &[after],
        format!(
            "否定の語が{}個から{}個になりました（前: {} / 後: {}）。肯定と否定、禁止と許可が入れ替わっていないか確認してください。",
            before_marks.len(),
            after_marks.len(),
            join_items(&before_marks),
            join_items(&after_marks)
        ),
    ));
}

// 確度と範囲を示す語。弱める語が消える、強める語が増えると主張が強まる。
const WEAKENERS: &[&str] = &[
    "未確認",
    "未定",
    "未決",
    "未検証",
    "検討中",
    "調査中",
    "確認中",
    "対応中",
    "作業中",
    "見込み",
    "予定",
    "暫定",
    "仮に",
    "仮の",
    "想定",
    "推定",
    "概算",
    "目安",
    "一部",
    "原則として",
    "基本的に",
    "おそらく",
    "恐らく",
    "たぶん",
    "多分",
    "おおむね",
    "概ね",
    "場合がある",
    "場合があります",
    "ことがある",
    "ことがあります",
    "傾向がある",
    "傾向があります",
];
const STRENGTHENERS: &[&str] = &[
    "確定",
    "必須",
    "確認済み",
    "対応済み",
    "検証済み",
    "必ず",
    "常に",
    "全て",
    "すべて",
    "絶対に",
    "確実に",
    "完全に",
];

/// `start`から始まる連続した形態素の表層が`word`と一致するか。「正常に」の中の「常に」のように
/// 語の途中から一致する形を除くため、文字列検索ではなく形態素の境界で照合する。
fn phrase_at(tokens: &[Morpheme], start: usize, word: &str) -> bool {
    let mut joined = String::new();
    for token in &tokens[start..] {
        joined.push_str(&token.surface);
        if joined.len() >= word.len() {
            return joined == word;
        }
        if !word.starts_with(joined.as_str()) {
            return false;
        }
    }
    false
}

fn certainty_words(units: &[&Unit]) -> Vec<String> {
    let mut found = Vec::new();
    for unit in units {
        for start in 0..unit.tokens.len() {
            for word in WEAKENERS.iter().chain(STRENGTHENERS) {
                if phrase_at(&unit.tokens, start, word) {
                    found.push((*word).to_owned());
                }
            }
        }
    }
    found
}

fn certainty_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let (lost, added) = multiset_diff(&certainty_words(before), &certainty_words(after));
    if lost.is_empty() && added.is_empty() {
        return;
    }
    let stronger = lost
        .iter()
        .filter(|word| WEAKENERS.contains(&word.as_str()))
        .chain(
            added
                .iter()
                .filter(|word| STRENGTHENERS.contains(&word.as_str())),
        )
        .cloned()
        .collect::<Vec<_>>();
    let weaker = added
        .iter()
        .filter(|word| WEAKENERS.contains(&word.as_str()))
        .chain(
            lost.iter()
                .filter(|word| STRENGTHENERS.contains(&word.as_str())),
        )
        .cloned()
        .collect::<Vec<_>>();
    findings.push(finding(
        "certainty_changed",
        "warn",
        before,
        after,
        format!(
            "確度や範囲を示す語が変わりました。主張が強まる変化: {} / 弱まる変化: {}。予定・暫定・一部などの留保を落としていないか、根拠なく断定を加えていないか確認してください。",
            join_items(&stronger),
            join_items(&weaker)
        ),
    ));
}

// 条件と限定。言い換え（場合→とき）は同じ種類として数える。
fn condition_markers(tokens: &[Morpheme]) -> (Vec<String>, Vec<String>) {
    let mut conditional = Vec::new();
    let mut restrictive = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let surface = token.surface.as_str();
        let previous = index.checked_sub(1).map(|index| &tokens[index]);
        let after_predicate =
            previous.is_some_and(|token| matches!(token.pos(0), "動詞" | "形容詞" | "助動詞"));
        let next = tokens.get(index + 1);
        // 「〜するとよい」の「と」は勧めの型として文末の働きで見る。
        let advice = surface == "と"
            && next
                .is_some_and(|token| matches!(token.dictionary_form(), "よい" | "いい" | "良い"));
        let is_conditional = match token.pos(0) {
            "助詞" => {
                // 「なければならない」「なくてはいけない」の「ば」は義務の型で、条件ではない。
                let obligation = surface == "ば"
                    && next.is_some_and(|token| {
                        matches!(token.dictionary_form(), "なる" | "いける" | "駄目" | "だめ")
                    });
                token.pos(1) == "接続助詞"
                    && matches!(surface, "ば" | "と")
                    && after_predicate
                    && !advice
                    && !obligation
            }
            "助動詞" => matches!(surface, "たら" | "だら" | "なら" | "ならば" | "れば"),
            "名詞" => {
                (matches!(surface, "場合" | "とき" | "際" | "限り" | "かぎり") && after_predicate)
                    || (surface == "時" && after_predicate)
            }
            _ => false,
        };
        if is_conditional {
            conditional.push(surface.to_owned());
            continue;
        }
        let is_restrictive = match token.pos(0) {
            "助詞" => matches!(surface, "だけ" | "のみ" | "しか" | "ばかり"),
            "名詞" | "接尾辞" => matches!(surface, "以外" | "例外" | "限定"),
            "動詞" => matches!(token.dictionary_form(), "除く" | "限る"),
            "接続詞" => matches!(surface, "ただし" | "但し"),
            _ => false,
        };
        if is_restrictive {
            restrictive.push(surface.to_owned());
        }
    }
    (conditional, restrictive)
}

fn condition_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let collect = |units: &[&Unit]| {
        let mut conditional = Vec::new();
        let mut restrictive = Vec::new();
        for unit in units {
            let (c, r) = condition_markers(&unit.tokens);
            conditional.extend(c);
            restrictive.extend(r);
        }
        (conditional, restrictive)
    };
    let (before_conditional, before_restrictive) = collect(before);
    let (after_conditional, after_restrictive) = collect(after);
    let mut changes = Vec::new();
    if before_conditional.len() != after_conditional.len() {
        changes.push(format!(
            "条件を表す語 {}→{}（前: {} / 後: {}）",
            before_conditional.len(),
            after_conditional.len(),
            join_items(&before_conditional),
            join_items(&after_conditional)
        ));
    }
    if before_restrictive.len() != after_restrictive.len() {
        changes.push(format!(
            "限定・例外を表す語 {}→{}（前: {} / 後: {}）",
            before_restrictive.len(),
            after_restrictive.len(),
            join_items(&before_restrictive),
            join_items(&after_restrictive)
        ));
    }
    if changes.is_empty() {
        return;
    }
    // 条件や限定が消えた変化は適用範囲が広がるので warn、増えただけなら info。
    let lost = before_conditional.len() > after_conditional.len()
        || before_restrictive.len() > after_restrictive.len();
    findings.push(finding(
        "condition_changed",
        if lost { "warn" } else { "info" },
        before,
        after,
        format!(
            "{}。適用される条件や例外が変わっていないか確認してください。",
            changes.join("。")
        ),
    ));
}

// 識別子。コード片、URL、英数字の語を大文字小文字を区別して比べる。
static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s）)」>]+").expect("valid URL regex"));
static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z_][A-Za-z0-9_]*(?:[.\-/:+][A-Za-z0-9_]+)*").expect("valid identifier regex")
});

fn identifiers(raw: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut remaining = raw.to_owned();
    for range in inline_code_ranges(raw) {
        found.push(raw[range.clone()].trim_matches('`').to_owned());
        remaining.replace_range(range.clone(), &" ".repeat(range.len()));
    }
    remaining = URL
        .replace_all(&remaining, |captures: &regex::Captures<'_>| {
            found.push(captures[0].to_owned());
            " ".repeat(captures[0].len())
        })
        .into_owned();
    // 英語の普通の単語（Markdown、Python）は内容語として比べ、ここでは略語・バージョン・
    // フラグ・ファイル名のように数字や記号を含む語と大文字だけの語を見る。
    found.extend(
        IDENTIFIER
            .find_iter(&remaining)
            .map(|found| found.as_str())
            .filter(|identifier| {
                identifier.chars().count() >= 2
                    && (identifier.chars().any(|ch| !ch.is_ascii_alphabetic())
                        || identifier.chars().all(|ch| ch.is_ascii_uppercase())
                        || identifier.chars().skip(1).any(|ch| ch.is_ascii_uppercase()))
            })
            .map(str::to_owned),
    );
    found
}

fn identifier_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let (lost, added) = multiset_diff(
        &identifiers(&block_raw(before)),
        &identifiers(&block_raw(after)),
    );
    findings.extend(item_change("identifier_changed", "warn", before, after, &lost, &added, |lost, added| {
        format!(
            "コード片・URL・英数字の語が変わりました。消えた語: {lost} / 増えた語: {added}。名前、フラグ、バージョンを書き換えていないか確認してください。"
        )
    }));
}

// 引用符の中身。引用を外しただけで本文に同じ文字列が残る場合は数えない。
static QUOTATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"「([^「」\n]+)」|『([^『』\n]+)』").expect("valid quotation regex")
});

fn quotations(raw: &str) -> Vec<String> {
    QUOTATION
        .captures_iter(raw)
        .filter_map(|captures| captures.get(1).or_else(|| captures.get(2)))
        .map(|found| crate::text::without_emphasis(found.as_str()))
        .collect()
}

fn quotation_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let before_raw = crate::text::without_emphasis(&block_raw(before));
    let after_raw = crate::text::without_emphasis(&block_raw(after));
    let (lost, added) = multiset_diff(&quotations(&before_raw), &quotations(&after_raw));
    let lost = lost
        .into_iter()
        .filter(|quote| !after_raw.contains(quote.as_str()))
        .collect::<Vec<_>>();
    let added = added
        .into_iter()
        .filter(|quote| !before_raw.contains(quote.as_str()))
        .collect::<Vec<_>>();
    findings.extend(item_change("quotation_changed", "warn", before, after, &lost, &added, |lost, added| {
        format!(
            "かぎ括弧の中身が変わりました。消えた引用: {lost} / 増えた引用: {added}。発言や用語の引用なら、原文のまま残してください。"
        )
    }));
}

// 引用の印。脚注・番号・著者と年の括弧・注番号。
static CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\[\^?[0-9A-Za-z_\-]{1,12}\]|",
        r"[（(][^（）()0-9\n]{1,30}[,，、 ] ?(?:et al\.?,? ?)?[0-9]{4}[a-z]?[）)]|",
        r"[注※][0-9０-９]+",
    ))
    .expect("valid citation regex")
});

fn citations(raw: &str) -> Vec<String> {
    CITATION
        .find_iter(raw)
        .filter(|found| {
            !found.as_str().starts_with('[') || !raw[found.end()..].starts_with(['(', ':', '（'])
        })
        .map(|found| found.as_str().to_owned())
        .collect()
}

fn citation_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let (lost, added) = multiset_diff(
        &citations(&block_raw(before)),
        &citations(&block_raw(after)),
    );
    findings.extend(item_change("citation_changed", "warn", before, after, &lost, &added, |lost, added| {
        format!(
            "引用や注の印が変わりました。消えた印: {lost} / 増えた印: {added}。出典の対応が崩れていないか確認してください。"
        )
    }));
}

// 主語・目的語。助詞の直前の名詞を正規化表記で比べる。
/// 助詞の直前の名詞。`form`は照合用の正規化表記、`surface`は表示用。
struct Argument {
    form: String,
    surface: String,
}

fn case_head(tokens: &[Morpheme], particle: usize) -> Option<Argument> {
    let head = tokens[..particle]
        .iter()
        .rev()
        .find(|token| !matches!(token.pos(0), "接尾辞" | "補助記号"))?;
    (head.pos(0) == "名詞"
        && !matches!(head.pos(1), "数詞")
        && !FORMAL_NOUNS.contains(&head.surface.as_str()))
    .then(|| Argument {
        form: head.normalized().to_owned(),
        surface: head.surface.clone(),
    })
}

fn case_arguments(tokens: &[Morpheme]) -> (Vec<Argument>, Vec<Argument>) {
    let mut subjects = Vec::new();
    let mut objects = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.pos(0) != "助詞" {
            continue;
        }
        let role = match (token.surface.as_str(), token.pos(1)) {
            ("が", "格助詞") | ("は", "係助詞") => &mut subjects,
            ("を", "格助詞") => &mut objects,
            _ => continue,
        };
        if let Some(head) = case_head(tokens, index) {
            role.push(head);
        }
    }
    (subjects, objects)
}

fn noun_forms(units: &[&Unit]) -> HashSet<String> {
    units
        .iter()
        .flat_map(|unit| unit.tokens.iter())
        .filter(|token| matches!(token.pos(0), "名詞" | "代名詞"))
        .map(|token| token.normalized().to_owned())
        .collect()
}

// 動作主になりやすい名詞の語尾。人・組織・システムを表す主語の追加だけを見る。
const AGENT_ENDINGS: &[&str] = &[
    "者",
    "員",
    "人",
    "手",
    "側",
    "チーム",
    "部",
    "課",
    "係",
    "社",
    "庁",
    "省",
    "局",
    "会",
    "班",
    "先",
    "主",
    "長",
    "師",
    "家",
    "士",
    "官",
    "客",
    "様",
    "氏",
    "ベンダー",
    "スタッフ",
    "メンバー",
    "ユーザー",
    "エージェント",
    "システム",
    "サーバー",
    "サービス",
    "ツール",
    "アプリ",
    "プログラム",
    "プロセス",
    "ジョブ",
    "バッチ",
    "モデル",
    "ボット",
    "AI",
    "CLI",
    "API",
    "SDK",
    "OS",
    "DB",
];
const NON_AGENT_SUBJECTS: &[&str] = &[
    "前者", "後者", "両者", "三者", "各者", "一部", "大部", "全部",
];

fn is_agent_like(tokens: &[Morpheme], subject: &Argument) -> bool {
    if NON_AGENT_SUBJECTS.contains(&subject.surface.as_str()) {
        return false;
    }
    let proper_noun = tokens
        .iter()
        .any(|token| token.surface == subject.surface && token.pos(1) == "固有名詞");
    proper_noun
        || AGENT_ENDINGS
            .iter()
            .any(|ending| subject.surface.ends_with(ending) || subject.form.ends_with(ending))
}

fn subject_added_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let known = noun_forms(before);
    let mut added = Vec::<String>::new();
    for unit in after {
        for subject in case_arguments(&unit.tokens).0 {
            if !known.contains(&subject.form)
                && is_agent_like(&unit.tokens, &subject)
                && !added.contains(&subject.surface)
            {
                added.push(subject.surface);
            }
        }
    }
    if added.is_empty() {
        return;
    }
    findings.push(finding(
        "subject_added",
        "warn",
        before,
        after,
        format!(
            "変更前にない主語が増えました: {}。担当者や動作主を原文や資料の裏付けなしに補っていないか確認してください。",
            join_items(&added)
        ),
    ));
}

/// 通知・報告・依頼の相手を改稿で補った場合の候補。
/// nwiizo-coding-style: 同じ節の明示的な「に／へ」と最初の動詞に限定する;
/// 省略や遠い係り受けの実例が集まったら反例とともに見直す。
fn recipient_added_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let known = noun_forms(before);
    for unit in after {
        let mut added = Vec::new();
        for (index, token) in unit.tokens.iter().enumerate() {
            if token.pos(1) != "格助詞" || !matches!(token.surface.as_str(), "に" | "へ") {
                continue;
            }
            let Some(actor) = case_head(&unit.tokens, index) else {
                continue;
            };
            if known.contains(&actor.form)
                || !is_agent_like(&unit.tokens, &actor)
                || !recipient_predicate(&unit.tokens[index + 1..])
            {
                continue;
            }
            // 照合は既存の名詞主要部、表示は「運用チーム」のような連続する名詞句。
            let mut start = index;
            while start > 0 && matches!(unit.tokens[start - 1].pos(0), "名詞" | "接頭辞" | "接尾辞")
            {
                start -= 1;
            }
            let surface = unit.tokens[start..index]
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<String>();
            if !added.contains(&surface) {
                added.push(surface);
            }
        }
        if !added.is_empty() {
            findings.push(finding(
                "recipient_added",
                "warn",
                before,
                &[unit],
                format!(
                    "変更前にない通知・報告・依頼の相手が増えました: {}。原文や資料に相手の指定があるか確認してください。",
                    join_items(&added)
                ),
            ));
        }
    }
}

fn recipient_predicate(tokens: &[Morpheme]) -> bool {
    let end = tokens
        .iter()
        .position(|token| {
            matches!(token.pos(0), "補助記号" | "記号")
                || (token.pos(0) == "助詞"
                    && matches!(
                        (token.surface.as_str(), token.pos(1)),
                        ("が", "格助詞") | ("は", "係助詞")
                    ))
        })
        .unwrap_or(tokens.len());
    let clause = &tokens[..end];
    let Some(index) = clause.iter().position(|token| token.pos(0) == "動詞") else {
        return false;
    };
    let verb = &clause[index];
    // Sudachiは「知らせる」を「知る」＋使役の「せる」に分ける。
    let shiraseru = verb.dictionary_form() == "知る"
        && clause
            .get(index + 1)
            .is_some_and(|token| token.dictionary_form() == "せる");
    let predicate_end = index + usize::from(shiraseru);
    if clause.get(predicate_end + 1).is_some_and(|token| {
        token.pos(0) == "助動詞"
            && matches!(
                token.dictionary_form(),
                "れる" | "られる" | "せる" | "させる"
            )
    }) {
        return false;
    }
    shiraseru
        || matches!(
            verb.dictionary_form(),
            "知らせる" | "伝える" | "送る" | "渡す" | "届ける"
        )
        || (verb.dictionary_form() == "する"
            && index.checked_sub(1).is_some_and(|previous| {
                matches!(
                    clause[previous].dictionary_form(),
                    "連絡" | "通知" | "報告" | "依頼" | "相談" | "共有" | "説明"
                )
            }))
}

fn nominalization_added_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"することが可能|が可能|の実施|を実施[しすさ]|を担保|を図[るりっられ]|に資(?:する|し|さ)|の観点から|に関して|を行[ういっわえ]"
    ).expect("valid nominalization regex")
    });
    let count = |units: &[&Unit]| {
        units
            .iter()
            .map(|unit| PATTERN.find_iter(&unit.text).count())
            .sum::<usize>()
    };
    let before_count = count(before);
    let after_count = count(after);
    if after_count > before_count {
        findings.push(finding("nominalization_added", "info", before, after, format!(
            "名詞化した言い回しが{before_count}件から{after_count}件へ増えました。短い動詞を「削減が可能」「実施」などへ替えて読みにくくしていないか確認してください。規程・論文の正式な語と、対象を保つために必要な名詞は残せます。"
        )));
    }
}

// 文末の働き。
struct EndingRule {
    kind: &'static str,
    pattern: LazyLock<Regex>,
}

macro_rules! ending_rule {
    ($kind:literal, $pattern:expr) => {
        EndingRule {
            kind: $kind,
            pattern: LazyLock::new(|| Regex::new($pattern).expect("valid ending regex")),
        }
    };
}

static ENDING_RULES: [EndingRule; 7] = [
    ending_rule!(
        "許可",
        r"(?:[てで](?:も)?(?:よい|いい|良い)(?:です)?|[てで](?:も)?(?:構わない|かまわない|構いません|かまいません|差し支えない|差し支えありません))(?:よ|ね)?$"
    ),
    ending_rule!(
        "依頼",
        r"(?:ください|下さい|いただけ(?:ます|ません|ると|れば|ますと|ないでしょう)|いただきたい|いただければ|お願い(?:し|いた)ます|お願いします|願います|てほしい|て欲しい|てもらえ(?:ます|ません)|頂け(?:ます|ません))(?:か|ね|よ)?$"
    ),
    ending_rule!(
        "禁止",
        r"(?:てはいけ(?:ない|ません)|てはなら(?:ない|ず|ません)|ないでください|ないで下さい|禁止(?:です|する|します|とする|とします)?|厳禁(?:です)?|べきで(?:は)?(?:ない|ありません)|てはだめ(?:です)?|ては駄目(?:です)?)$"
    ),
    ending_rule!(
        "義務",
        r"(?:なければ(?:ならない|なりません|いけない|いけません)|なくては(?:ならない|なりません|いけない|いけません)|ないと(?:いけない|いけません|ならない|なりません)|ねばなら(?:ない|ぬ)|ざるを得(?:ない|ません)|必要(?:が|は)?(?:ある|あります|です|だ|である|となる|になる|になります|となります)|必須(?:です|だ|である|となる|とする|とします)?|べき(?:だ|です|である|でしょう)?|義務(?:がある|があります|づけられる|づけられます)|要する|要します|求められ(?:る|ます))$"
    ),
    ending_rule!(
        "勧め",
        r"(?:ましょう|(?:と|が|ほうが|方が)(?:よい|いい|良い)(?:です|でしょう)?|望ましい(?:です)?|(?:推奨|お勧め|おすすめ|お薦め)(?:します|です|する|である|だ|いたします|されます|されています)?|勧め(?:ます|る|られます|られる))$"
    ),
    ending_rule!(
        "可能",
        r"(?:でき(?:る|ます)|ことが(?:でき|出来)(?:る|ます)|出来(?:る|ます)|可能(?:です|だ|である|となる|になる|になります|となります)?|得(?:る|ます)|うる)$"
    ),
    ending_rule!(
        "推量",
        r"(?:でしょう|だろう|かもしれ(?:ない|ません)|と(?:思い|考え|感じ)ます|と(?:思う|考える|考えている|思っている|考えています|思っています)|と(?:思わ|考えら|見ら|みら)れ(?:る|ます|ています|ている)|はず(?:です|だ|である)?|(?:よう|みたい)(?:です|だ|である)|らしい(?:です)?|気がし(?:ます|た|ている|ています)|可能性が(?:ある|あります|高い|高いです|低い|低いです)|(?:恐れ|おそれ|懸念)が(?:ある|あります)|見込み(?:です|だ|である)?|と(?:推測|推定|予想|想定)(?:され|し)(?:る|ます|ています|ている)|のでは(?:ない|ありません)(?:か|でしょうか)|想定です|と(?:みられ|見られ|みえ|見え)(?:る|ます))$"
    ),
];

static CITATION_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\[\^?[0-9A-Za-z_\-]{1,12}\]|[注※][0-9０-９]+)$")
        .expect("valid citation tail regex")
});

/// 文末の判定に使う範囲。末尾の句読点、閉じ括弧、引用の印を外す。
fn ending_tail(text: &str) -> &str {
    let mut tail = text;
    loop {
        let trimmed = tail.trim_end_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '。' | '．'
                        | '.'
                        | '！'
                        | '!'
                        | '？'
                        | '?'
                        | '」'
                        | '』'
                        | '）'
                        | ')'
                        | '"'
                        | '”'
                )
        });
        let trimmed = CITATION_TAIL
            .find(trimmed)
            .map_or(trimmed, |found| &trimmed[..found.start()]);
        if trimmed.len() == tail.len() {
            return tail;
        }
        tail = trimmed;
    }
}

fn ending_kind(unit: &Unit) -> &'static str {
    let tail = ending_tail(&unit.text);
    let tokens = &unit.tokens[..unit
        .tokens
        .partition_point(|token| token.byte_end <= tail.len())];
    let last = tokens
        .iter()
        .rev()
        .find(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"));
    let ends_with_question_particle =
        last.is_some_and(|token| token.pos(1) == "終助詞" && token.surface == "か");
    if matches!(unit.end_mark, Some('？' | '?')) || ends_with_question_particle {
        return "疑問";
    }
    for rule in &ENDING_RULES {
        if let Some(found) = rule.pattern.find(tail) {
            // 「とてもよい」の副詞の一部を、動作の許可「てもよい」と取り違えない。
            if rule.kind == "許可"
                && !tokens.iter().any(|token| {
                    token.byte_start == found.start()
                        && token.pos(1) == "接続助詞"
                        && matches!(token.surface.as_str(), "て" | "で")
                })
            {
                continue;
            }
            return rule.kind;
        }
    }
    let Some(last) = last else {
        return "体言止め";
    };
    let position = tokens.iter().rposition(|token| {
        !matches!(token.pos(0), "記号" | "補助記号" | "空白")
            && !(token.pos(1) == "終助詞" && matches!(token.surface.as_str(), "ね" | "よ" | "な"))
    });
    let last = position.map_or(last, |position| &tokens[position]);
    if unit.potential_tail {
        return "可能";
    }
    // 「見られる」「送られる」の「れる／られる」は受身・可能・尊敬のどれにも読めるため、まとめて示す。
    let start = tail_start(&unit.text);
    if tokens.iter().any(|token| {
        token.byte_start >= start
            && token.pos(0) == "助動詞"
            && matches!(token.dictionary_form(), "れる" | "られる")
    }) {
        return "受身・可能";
    }
    // 「読んだ」の「だ」は辞書形が「だ」になることがあるため、動詞に続く「た／だ」を過去と見る。
    let after_verb = position
        .and_then(|position| position.checked_sub(1))
        .is_some_and(|previous| tokens[previous].pos(0) == "動詞");
    if last.pos(0) == "助動詞"
        && (last.dictionary_form() == "た"
            || (after_verb && matches!(last.surface.as_str(), "た" | "だ")))
    {
        return "過去";
    }
    if matches!(
        last.pos(0),
        "名詞" | "代名詞" | "形状詞" | "接尾辞" | "助詞" | "副詞" | "連体詞"
    ) {
        return "体言止め";
    }
    "説明・断定"
}

/// 対応する文の対について、文末の働きの変化と主語・目的語の入れ替わりを見る。
/// 対応のない文は、内容語が相手側に残っていなければ削除・追加として示す。
fn pair_findings(
    before: &[&Unit],
    after: &[&Unit],
    claimed_by: &[Option<usize>],
    findings: &mut Vec<DiffFinding>,
) {
    for (b_index, b_unit) in before.iter().enumerate() {
        let group = claimed_by
            .iter()
            .enumerate()
            .filter(|(_, claimed)| **claimed == Some(b_index))
            .map(|(a_index, _)| a_index)
            .collect::<Vec<_>>();
        let Some(&last_index) = group.last() else {
            continue;
        };
        let last_unit = after[last_index];
        let commas_before = b_unit.duplicate_commas;
        let commas_after = last_unit.duplicate_commas;
        if commas_after > commas_before && last_unit.kind != UnitKind::Quote {
            findings.push(finding(
                "duplicate_comma_added",
                "warn",
                &[b_unit],
                &[last_unit],
                "改稿後に連続する読点が増えました。修正時の重複であれば一つに戻してください。"
                    .into(),
            ));
        }
        negation_findings(b_unit, last_unit, findings);
        if group.len() == 1 {
            style_findings(b_unit, last_unit, findings);
        }
        structure_findings(b_unit, last_unit, findings);
        contrast_findings(b_unit, last_unit, findings);
        causal_findings(b_unit, last_unit, findings);
        if b_unit.kind != UnitKind::Heading
            && last_unit.kind != UnitKind::Heading
            && (b_unit.ability_text != last_unit.ability_text
                || b_unit.end_mark != last_unit.end_mark)
        {
            let (before_kind, after_kind) = (ending_kind(b_unit), ending_kind(last_unit));
            // 体言止めを文にした変化は文体の調整として扱い、時制だけの変化と、受身・可能・尊敬の
            // どれにも読める「れる／られる」との行き来は info にする。
            let unmarked =
                |kind: &str| matches!(kind, "説明・断定" | "過去" | "体言止め" | "受身・可能");
            let severity = match (before_kind, after_kind) {
                (b, a) if b == a => None,
                (b, a) if b == "体言止め" || a == "体言止め" => {
                    (!unmarked(b) || !unmarked(a)).then_some("warn")
                }
                (b, a) if unmarked(b) && unmarked(a) => Some("info"),
                _ => Some("warn"),
            };
            if let Some(severity) = severity {
                findings.push(finding(
                    "sentence_function_changed",
                    severity,
                    &[b_unit],
                    &[last_unit],
                    format!(
                        "文末の働きが「{before_kind}」から「{after_kind}」に変わりました。依頼、許可、できること、自動で起きること、推量の区別を原文のとおりに保ってください。"
                    ),
                ));
            }
        }
        let first_unit = after[group[0]];
        let (before_subjects, before_objects) = case_arguments(&b_unit.tokens);
        let (after_subjects, after_objects) = case_arguments(&first_unit.tokens);
        let has = |arguments: &[Argument], form: &str| {
            arguments.iter().any(|argument| argument.form == form)
        };
        let swapped = before_subjects
            .iter()
            .filter(|subject| has(&after_objects, &subject.form))
            .find_map(|subject| {
                before_objects
                    .iter()
                    .find(|object| {
                        object.form != subject.form && has(&after_subjects, &object.form)
                    })
                    .map(|object| (subject.surface.clone(), object.surface.clone()))
            });
        if let Some((subject, object)) = swapped {
            findings.push(finding(
                "subject_object_swapped",
                "warn",
                &[b_unit],
                &[first_unit],
                format!(
                    "「{subject}」と「{object}」の主語・目的語が入れ替わっています。動作の向きが原文と同じか確認してください。"
                ),
            ));
        }
    }
}

/// 箇条書きと地の文の入れ替わり、見出しの階層の変化。
fn structure_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let detail = match (before.kind, after.kind) {
        (UnitKind::List, UnitKind::Prose) => {
            "箇条書きの項目が地の文になりました。並列の走査性が要る一覧なら箇条書きのまま残してください。".to_owned()
        }
        (UnitKind::Prose, UnitKind::List) => {
            "地の文が箇条書きになりました。因果や経緯が項目の間に隠れていないか確認してください。".to_owned()
        }
        (UnitKind::Heading, UnitKind::Heading) if before.level != after.level => format!(
            "見出しの階層が{}から{}に変わりました。節の包含関係が意図どおりか確認してください。",
            "#".repeat(before.level),
            "#".repeat(after.level)
        ),
        _ => return,
    };
    findings.push(finding(
        "structure_changed",
        "info",
        &[before],
        &[after],
        detail,
    ));
}

// 「XではなくY」の否定対比。
static CONTRAST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([^、。「」（）\s]{1,20}?)(?:では|じゃ|で)な(?:く|くて)、?\s*([^、。\s]{1,20})")
        .expect("valid contrast regex")
});

fn contrasts(text: &str) -> Vec<(String, String)> {
    CONTRAST
        .captures_iter(text)
        .map(|captures| (captures[1].to_owned(), captures[2].to_owned()))
        .collect()
}

fn overlaps(left: &str, right: &str) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

/// 否定対比の逆転（warn）と、否定した側の消失（info）。
fn contrast_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let content = |text: &str| {
        text.chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '、')
            .collect::<String>()
    };
    if content(&before.text) == content(&after.text) {
        return;
    }
    let before_pairs = contrasts(&before.text);
    if before_pairs.is_empty() {
        return;
    }
    let after_pairs = contrasts(&after.text);
    for (denied, chosen) in &before_pairs {
        let reversed = after_pairs.iter().find(|(denied_after, chosen_after)| {
            overlaps(denied_after, chosen) && overlaps(chosen_after, denied)
        });
        if let Some((denied_after, chosen_after)) = reversed {
            findings.push(finding(
                "contrast_changed",
                "warn",
                &[before],
                &[after],
                format!(
                    "否定対比の向きが逆になっています。前は「{denied}」を否定して「{chosen}」を選び、後は「{denied_after}」を否定して「{chosen_after}」を選んでいます。"
                ),
            ));
            return;
        }
        if after_pairs.is_empty() && !after.text.contains(denied.as_str()) {
            findings.push(finding(
                "contrast_changed",
                "info",
                &[before],
                &[after],
                format!(
                    "「{denied}ではなく{chosen}」の否定した側「{denied}」が消えました。誤解の訂正や比較として必要な対比なら残してください。"
                ),
            ));
            return;
        }
    }
}

// 因果の接続。
fn causal_markers(tokens: &[Morpheme]) -> Vec<String> {
    let mut marks = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let surface = token.surface.as_str();
        let previous = index.checked_sub(1).map(|index| &tokens[index]);
        let next = tokens.get(index + 1);
        let after_predicate =
            previous.is_some_and(|token| matches!(token.pos(0), "動詞" | "形容詞" | "助動詞"));
        let causal = match token.pos(0) {
            "助詞" => {
                token.pos(1) == "接続助詞" && matches!(surface, "ので" | "から") && after_predicate
            }
            "名詞" => {
                (matches!(surface, "ため" | "為")
                    && (after_predicate || previous.is_some_and(|token| token.surface == "の")))
                    || matches!(surface, "おかげ" | "せい" | "原因" | "要因")
            }
            "接続詞" => matches!(
                surface,
                "そのため"
                    | "このため"
                    | "したがって"
                    | "従って"
                    | "だから"
                    | "なぜなら"
                    | "よって"
                    | "ゆえに"
                    | "その結果"
            ),
            "動詞" => {
                token.dictionary_form() == "よる"
                    && previous.is_some_and(|token| token.surface == "に")
                    && !next.is_some_and(|token| token.surface == "と")
            }
            _ => false,
        };
        if causal {
            marks.push(if token.dictionary_form() == "よる" {
                format!("に{surface}")
            } else {
                surface.to_owned()
            });
        }
    }
    marks
}

fn causal_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let before_marks = causal_markers(&before.tokens);
    let after_marks = causal_markers(&after.tokens);
    if before_marks.len() == after_marks.len() {
        return;
    }
    let direction = if after_marks.len() > before_marks.len() {
        "原文にない因果のつながりが増えました"
    } else {
        "因果のつながりが減りました"
    };
    findings.push(finding(
        "causal_changed",
        "info",
        &[before],
        &[after],
        format!(
            "{direction}（前: {} / 後: {}）。原因と結果の関係に原文や資料の裏付けがあるか確認してください。",
            join_items(&before_marks),
            join_items(&after_marks)
        ),
    ));
}

// 気持ち・評価の語と、書き手の約束。
const FEELING_WORDS: &[&str] = &[
    "嬉しい",
    "うれしい",
    "嬉しく",
    "楽しい",
    "楽しみ",
    "悲しい",
    "残念",
    "安心",
    "不安",
    "心配",
    "感謝",
    "ありがたい",
    "ありがとう",
    "申し訳",
    "驚き",
    "驚く",
    "感動",
    "誇り",
    "悔しい",
    "期待",
    "幸い",
    "幸せ",
    "喜び",
    "喜ぶ",
    "怒り",
    "怖い",
    "恐ろしい",
    "痛感",
    "実感",
    "心から",
    "切に",
    "ぜひ",
    "是非",
    "歓迎",
    "光栄",
    "恐縮",
    "恐れ入り",
    "寂しい",
    "辛い",
    "つらい",
    "面白い",
    "興味深い",
    "素晴らしい",
    "すばらしい",
    "魅力的",
];
static PROMISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"いたします|致します|てまいります|て参ります|徹底します|お約束|約束します|二度と|",
        r"再発防止に努め|努めます|努めてまいります|取り組んでまいります|保証します|お詫び",
    ))
    .expect("valid promise regex")
});

fn phrase_set(units: &[&Unit], words: &[&str]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for unit in units {
        for start in 0..unit.tokens.len() {
            for word in words {
                if phrase_at(&unit.tokens, start, word) {
                    found.insert((*word).to_owned());
                }
            }
        }
    }
    found
}

fn feeling_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let before_words = phrase_set(before, FEELING_WORDS);
    let added = phrase_set(after, FEELING_WORDS)
        .difference(&before_words)
        .cloned()
        .collect::<Vec<_>>();
    if added.is_empty() {
        return;
    }
    findings.push(finding(
        "feeling_added",
        "info",
        before,
        after,
        format!(
            "変更前にない気持ち・評価の語が増えました: {}。書き手の感情や評価を推敲で作り足していないか確認してください。",
            join_items(&added)
        ),
    ));
}

fn promise_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let promised = |units: &[&Unit]| {
        units
            .iter()
            .flat_map(|unit| {
                PROMISE
                    .find_iter(&unit.text)
                    .map(|found| found.as_str().to_owned())
            })
            .collect::<Vec<_>>()
    };
    if !promised(before).is_empty() {
        return;
    }
    let added = promised(after);
    if added.is_empty() {
        return;
    }
    findings.push(finding(
        "promise_added",
        "info",
        before,
        after,
        format!(
            "変更前にない書き手の約束や誓いの言い回しが増えました: {}。原文にない保証や再発防止の約束を加えていないか確認してください。",
            join_items(&added)
        ),
    ));
}

// 敬体と常体。
static POLITE_END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:です|ます|ません|ました|でした|ましょう|ください|でしょう|ございます)(?:か|ね|よ)?$",
    )
    .expect("valid polite ending regex")
});

/// 文が敬体ならSome(true)、常体ならSome(false)。体言止めや見出しはNone。
fn unit_register(unit: &Unit) -> Option<bool> {
    if !matches!(unit.kind, UnitKind::Prose | UnitKind::List) {
        return None;
    }
    let tail = ending_tail(&unit.text);
    if POLITE_END.is_match(tail) {
        return Some(true);
    }
    let last = unit.tokens.iter().rev().find(|token| {
        token.byte_end <= tail.len() && !matches!(token.pos(0), "記号" | "補助記号" | "空白")
    });
    last.is_some_and(|token| {
        matches!(token.pos(0), "動詞" | "形容詞")
            || (token.pos(0) == "助動詞"
                && matches!(token.dictionary_form(), "だ" | "た" | "ない" | "ぬ" | "ず"))
    })
    .then_some(false)
}

/// 敬体と常体に分けられる文の数。体言止めや見出しは数えない。
fn register_counts(units: &[Unit]) -> (usize, usize) {
    let registers = units.iter().filter_map(unit_register).collect::<Vec<_>>();
    let polite = registers.iter().filter(|polite| **polite).count();
    (polite, registers.len() - polite)
}

fn register_findings(before: &[Unit], after: &[Unit]) -> Vec<DiffFinding> {
    const MIN_SENTENCES: usize = 5;
    const SHIFT: f64 = 0.2;
    let (before_polite, before_plain) = register_counts(before);
    let (after_polite, after_plain) = register_counts(after);
    let before_total = before_polite + before_plain;
    let after_total = after_polite + after_plain;
    if before_total < MIN_SENTENCES || after_total < MIN_SENTENCES {
        return Vec::new();
    }
    let before_ratio = before_polite as f64 / before_total as f64;
    let after_ratio = after_polite as f64 / after_total as f64;
    if (before_ratio - after_ratio).abs() < SHIFT {
        return Vec::new();
    }
    vec![DiffFinding {
        category: "register_changed".to_owned(),
        severity: "info".to_owned(),
        before_line: before.first().map(|unit| unit.line),
        after_line: after.first().map(|unit| unit.line),
        before_excerpt: format!("敬体 {before_polite}文 / 常体 {before_plain}文"),
        after_excerpt: format!("敬体 {after_polite}文 / 常体 {after_plain}文"),
        detail: format!(
            "敬体の割合が{:.0}%から{:.0}%に変わりました。文書全体の文体を変える意図か、読者と媒体に合うかを確認してください。",
            before_ratio * 100.0,
            after_ratio * 100.0
        ),
    }]
}

/// 文字2連の一致率と内容語の一致率の高いほう。短い文は内容語で対応づける。
fn pair_score(left: &Unit, right: &Unit) -> f64 {
    let lemma_union = left.lemmas.union(&right.lemmas).count();
    let lemma_score = if lemma_union == 0 {
        0.0
    } else {
        left.lemmas.intersection(&right.lemmas).count() as f64 / lemma_union as f64
    };
    similarity(left, right).max(if lemma_score >= 0.5 { lemma_score } else { 0.0 })
}

fn coverage(unit: &Unit, block: &[&Unit]) -> f64 {
    if unit.lemmas.is_empty() {
        return 1.0;
    }
    let kept = unit
        .lemmas
        .iter()
        .filter(|lemma| block.iter().any(|other| other.lemmas.contains(*lemma)))
        .count();
    kept as f64 / unit.lemmas.len() as f64
}

const LIGHT_VERBS: &[&str] = &[
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

fn content_lemmas(tokens: &[Morpheme]) -> Vec<String> {
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

fn content_word_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let before_lemmas = before
        .iter()
        .flat_map(|unit| unit.ability_lemmas.iter().cloned())
        .collect::<HashSet<_>>();
    let after_lemmas = after
        .iter()
        .flat_map(|unit| unit.ability_lemmas.iter().cloned())
        .collect::<HashSet<_>>();
    let mut lost = before_lemmas
        .difference(&after_lemmas)
        .cloned()
        .collect::<Vec<_>>();
    let mut added = after_lemmas
        .difference(&before_lemmas)
        .cloned()
        .collect::<Vec<_>>();
    lost.sort();
    added.sort();
    findings.extend(item_change("content_words_changed", "info", before, after, &lost, &added, |lost, added| {
        format!(
            "書き換えで消えた語: {lost} / 増えた語: {added}。消えた語が条件や対象を担っていないか、増えた語に原文や資料の裏付けがあるか確認してください。"
        )
    }));
}

// ---- 文書全体で比べる保護範囲 ----

fn fenced_blocks(raw: &str) -> Vec<LinedText> {
    let lines = raw.split('\n').collect::<Vec<_>>();
    let fenced = fenced_lines(&lines);
    let mut blocks = Vec::new();
    let mut current: Option<(usize, Vec<&str>)> = None;
    for (index, line) in lines.iter().enumerate() {
        match (&mut current, fenced[index]) {
            (None, true) => current = Some((index + 1, vec![line])),
            (Some((_, body)), true) => {
                body.push(line);
                if body.len() > 1 && line.trim_start().starts_with(['`', '~']) {
                    let (start, body) = current.take().expect("open block");
                    blocks.push((start, body.join("\n")));
                }
            }
            _ => {}
        }
    }
    if let Some((start, body)) = current {
        blocks.push((start, body.join("\n")));
    }
    blocks
}

fn table_rows(raw: &str) -> Vec<LinedText> {
    let lines = raw.split('\n').collect::<Vec<_>>();
    let fenced = fenced_lines(&lines);
    lines
        .iter()
        .enumerate()
        .filter(|(index, _)| !fenced[*index])
        .map(|(index, line)| (index + 1, line.trim()))
        .filter(|(_, line)| {
            line.starts_with('|')
                && line.matches('|').count() >= 2
                && !line.chars().all(|ch| matches!(ch, '|' | '-' | ':' | ' '))
        })
        .map(|(line_no, line)| (line_no, line.to_owned()))
        .collect()
}

/// 行番号付きの本文（コードブロック1件、表の1行）。
type LinedText = (usize, String);

fn lined_diff(before: &[LinedText], after: &[LinedText]) -> (Vec<LinedText>, Vec<LinedText>) {
    let mut after_counts = HashMap::<&str, usize>::new();
    for (_, body) in after {
        *after_counts.entry(body.as_str()).or_default() += 1;
    }
    let mut before_counts = HashMap::<&str, usize>::new();
    for (_, body) in before {
        *before_counts.entry(body.as_str()).or_default() += 1;
    }
    let take = |counts: &mut HashMap<&str, usize>, body: &str| {
        counts
            .get_mut(body)
            .filter(|count| **count > 0)
            .map(|count| *count -= 1)
            .is_some()
    };
    let lost = before
        .iter()
        .filter(|(_, body)| !take(&mut after_counts, body))
        .cloned()
        .collect::<Vec<_>>();
    let added = after
        .iter()
        .filter(|(_, body)| !take(&mut before_counts, body))
        .cloned()
        .collect::<Vec<_>>();
    (lost, added)
}

fn protected_findings(before: &str, after: &str) -> Vec<DiffFinding> {
    let mut findings = Vec::new();
    let (lost_blocks, added_blocks) = lined_diff(&fenced_blocks(before), &fenced_blocks(after));
    if !lost_blocks.is_empty() || !added_blocks.is_empty() {
        let excerpt_of = |blocks: &[(usize, String)]| {
            blocks
                .first()
                .map(|(_, body)| {
                    let mut text = body.chars().take(EXCERPT_CHARS).collect::<String>();
                    if body.chars().count() > EXCERPT_CHARS {
                        text.push('…');
                    }
                    text.replace('\n', "⏎")
                })
                .unwrap_or_default()
        };
        findings.push(DiffFinding {
            category: "protected_changed".to_owned(),
            severity: "warn".to_owned(),
            before_line: lost_blocks.first().map(|(line, _)| *line),
            after_line: added_blocks.first().map(|(line, _)| *line),
            before_excerpt: excerpt_of(&lost_blocks),
            after_excerpt: excerpt_of(&added_blocks),
            detail: format!(
                "コードブロックが変わりました（消えた{}件、増えた{}件）。推敲ではコード、コマンド、出力例を書き換えないでください。",
                lost_blocks.len(),
                added_blocks.len()
            ),
        });
    }
    findings.extend(table_findings(before, after));
    findings
}

fn text_similarity(left: &str, right: &str) -> f64 {
    let left_set = bigrams(left);
    let right_set = bigrams(right);
    let total = left_set.len() + right_set.len();
    if total == 0 {
        return 0.0;
    }
    2.0 * left_set.intersection(&right_set).count() as f64 / total as f64
}

/// 表の行を字面の近さで対応づけ、数値と識別子の変化を行ごとに示す。対応のない行は件数だけ示す。
fn table_findings(before: &str, after: &str) -> Vec<DiffFinding> {
    let (lost_rows, added_rows) = lined_diff(&table_rows(before), &table_rows(after));
    let mut findings = Vec::new();
    let mut used = vec![false; added_rows.len()];
    let mut unpaired_lost = 0;
    for (before_line, before_row) in &lost_rows {
        let best = added_rows
            .iter()
            .enumerate()
            .filter(|(index, _)| !used[*index])
            .map(|(index, (_, row))| (index, text_similarity(before_row, row)))
            .filter(|(_, score)| *score >= PAIR_SIMILARITY)
            .max_by(|left, right| left.1.total_cmp(&right.1).then(right.0.cmp(&left.0)));
        let Some((index, _)) = best else {
            unpaired_lost += 1;
            continue;
        };
        used[index] = true;
        let (after_line, after_row) = &added_rows[index];
        let (lost_numbers, added_numbers) = multiset_diff(
            &distinct(numbers(before_row)),
            &distinct(numbers(after_row)),
        );
        let (lost_identifiers, added_identifiers) =
            multiset_diff(&identifiers(before_row), &identifiers(after_row));
        let value_changed = !(lost_numbers.is_empty()
            && added_numbers.is_empty()
            && lost_identifiers.is_empty()
            && added_identifiers.is_empty());
        let detail = if value_changed {
            format!(
                "表の行の数値や識別子が変わりました。消えた値: {} / 増えた値: {}。セルの値と単位が原文と同じか確認してください。",
                join_items(&[lost_numbers, lost_identifiers].concat()),
                join_items(&[added_numbers, added_identifiers].concat())
            )
        } else {
            "表の行の文言が変わりました。項目名や説明の意味が原文と同じか確認してください。"
                .to_owned()
        };
        findings.push(DiffFinding {
            category: "table_changed".to_owned(),
            severity: if value_changed { "warn" } else { "info" }.to_owned(),
            before_line: Some(*before_line),
            after_line: Some(*after_line),
            before_excerpt: before_row.clone(),
            after_excerpt: after_row.clone(),
            detail,
        });
    }
    let unpaired_added = used.iter().filter(|used| !**used).count();
    if unpaired_lost > 0 || unpaired_added > 0 {
        let first_lost = lost_rows.iter().find(|(_, row)| {
            !added_rows
                .iter()
                .enumerate()
                .any(|(index, (_, candidate))| used[index] && candidate == row)
        });
        let first_added = added_rows
            .iter()
            .zip(&used)
            .find(|(_, used)| !**used)
            .map(|(row, _)| row);
        findings.push(DiffFinding {
            category: "table_changed".to_owned(),
            severity: "info".to_owned(),
            before_line: first_lost.map(|(line, _)| *line),
            after_line: first_added.map(|(line, _)| *line),
            before_excerpt: first_lost.map(|(_, row)| row.clone()).unwrap_or_default(),
            after_excerpt: first_added.map(|(_, row)| row.clone()).unwrap_or_default(),
            detail: format!(
                "表の行が増減しました（対応のない行: 消えた{unpaired_lost}行、増えた{unpaired_added}行）。消えた行の値や条件が本文に移っているか確認してください。"
            ),
        });
    }
    findings
}

// ---- 改稿で気づかずに変わる文体と、変えてはいけない値 ----

fn is_kanji(ch: char) -> bool {
    matches!(ch, '一'..='龥' | '々' | '〆')
}

/// 1対1で対応した文の、読点・漢字の割合・敬体と常体・見出しの文言の変化。
fn style_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    if before.kind == UnitKind::Quote || after.kind == UnitKind::Quote || before.key == after.key {
        return;
    }
    if before.kind == UnitKind::Heading && after.kind == UnitKind::Heading {
        findings.push(finding(
            "heading_rewritten",
            "info",
            &[before],
            &[after],
            "見出しの文言が変わりました。目次、リンクのアンカー、ほかの文書からの参照が切れないか、依頼で見出しの形を指定されていないか確認してください。".to_owned(),
        ));
        return;
    }
    let commas = |unit: &Unit| unit.text.matches(['、', '，']).count();
    let added_commas = commas(after).saturating_sub(commas(before));
    if added_commas >= 2 {
        findings.push(finding(
            "comma_added",
            "info",
            &[before],
            &[after],
            format!(
                "読点が{added_commas}個増えました。主題や目的語の直後の読点で文が細切れになっていないか、原文の区切りで読めるか確認してください。"
            ),
        ));
    }
    let measure = |text: &str| {
        let counted = text.chars().filter(|ch| !ch.is_whitespace()).count();
        let kanji = text.chars().filter(|ch| is_kanji(*ch)).count();
        let hiragana = text.chars().filter(|ch| matches!(ch, 'ぁ'..='ゖ')).count();
        (counted.max(1), kanji, hiragana)
    };
    let (before_chars, before_kanji, before_hiragana) = measure(&before.text);
    let (after_chars, after_kanji, after_hiragana) = measure(&after.text);
    let ratio = |kanji: usize, chars: usize| kanji as f64 * 100.0 / chars as f64;
    let (before_ratio, after_ratio) = (
        ratio(before_kanji, before_chars),
        ratio(after_kanji, after_chars),
    );
    if after_ratio - before_ratio >= 8.0
        && after_kanji >= before_kanji + 2
        && before_hiragana >= after_hiragana + 2
    {
        findings.push(finding(
            "stiffness_increased",
            "info",
            &[before],
            &[after],
            format!(
                "漢字の割合が{before_ratio:.0}%から{after_ratio:.0}%に増え、仮名が減りました。和語の動詞を漢語の名詞に置き換えて硬くなっていないか、読者に合うか確認してください。"
            ),
        ));
    }
    if let (Some(before_polite), Some(after_polite)) = (unit_register(before), unit_register(after))
        && before_polite != after_polite
    {
        let name = |polite: bool| if polite { "敬体" } else { "常体" };
        findings.push(finding(
            "sentence_register_changed",
            "info",
            &[before],
            &[after],
            format!(
                "この文だけが{}から{}に変わりました。前後の文と文体がそろっているか確認してください。",
                name(before_polite),
                name(after_polite)
            ),
        ));
    }
}

static ACTOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[一-龥々ァ-ヴー]*(?:担当者|責任者|管理者|利用者|担当|チーム)")
        .expect("valid actor regex")
});

/// 「担当者」が「運用担当者」になったように、人や担当の語が狭まった箇所。
fn actor_narrowed_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
    let words = |units: &[&Unit]| {
        units
            .iter()
            .flat_map(|unit| {
                ACTOR
                    .find_iter(&unit.text)
                    .map(|found| found.as_str().to_owned())
            })
            .collect::<BTreeSet<_>>()
    };
    let (before_words, after_words) = (words(before), words(after));
    fn containing<'a>(units: &[&'a Unit], word: &str) -> Vec<&'a Unit> {
        units
            .iter()
            .find(|unit| unit.text.contains(word))
            .copied()
            .into_iter()
            .collect()
    }
    for old in before_words.difference(&after_words) {
        let Some(new) = after_words
            .difference(&before_words)
            .find(|new| new.ends_with(old.as_str()) && new.len() > old.len())
        else {
            continue;
        };
        findings.push(finding(
            "actor_narrowed",
            "warn",
            &containing(before, old),
            &containing(after, new),
            format!(
                "人や担当を指す語が「{old}」から「{new}」に狭まりました。原文や資料で担当の範囲が決まっているか確認してください。推測で特定の担当に絞らないでください。"
            ),
        ));
    }
}

const INTENSIFIERS: &[&str] = &[
    "本当に",
    "とても",
    "すごく",
    "ぜひ",
    "絶対",
    "最強",
    "抜群",
    "イチオシ",
    "大活躍",
];

/// 感嘆・疑問の記号、絵文字、強めの語が文書全体で半分以下に減ったか。
fn tone_findings(before: &[Unit], after: &[Unit]) -> Vec<DiffFinding> {
    let count = |units: &[Unit], kind: usize| -> usize {
        units
            .iter()
            .filter(|unit| unit.kind != UnitKind::Quote)
            .map(|unit| match kind {
                0 => {
                    unit.raw_text.matches(['！', '!']).count()
                        + usize::from(matches!(unit.end_mark, Some('！' | '!')))
                }
                1 => {
                    unit.raw_text.matches(['？', '?']).count()
                        + usize::from(matches!(unit.end_mark, Some('？' | '?')))
                }
                2 => unit
                    .raw_text
                    .chars()
                    .filter(|ch| matches!(*ch as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF))
                    .count(),
                _ => INTENSIFIERS
                    .iter()
                    .map(|word| unit.text.matches(word).count())
                    .sum(),
            })
            .sum()
    };
    let names = ["感嘆符", "疑問符", "絵文字", "強めの語"];
    let lost = (0..names.len())
        .filter_map(|kind| {
            let (old, new) = (count(before, kind), count(after, kind));
            (old >= 2 && new * 2 <= old).then(|| format!("{} {old}→{new}", names[kind]))
        })
        .collect::<Vec<_>>();
    if lost.is_empty() {
        return Vec::new();
    }
    vec![DiffFinding {
        category: "tone_flattened".to_owned(),
        severity: "info".to_owned(),
        before_line: before.first().map(|unit| unit.line),
        after_line: after.first().map(|unit| unit.line),
        before_excerpt: String::new(),
        after_excerpt: String::new(),
        detail: format!(
            "文書の勢いを表す記号や語が半分以下に減りました（{}）。宣伝文・告知・会話のように勢いを残す文書なら、書き手の調子を保っているか確認してください。",
            lost.join("、")
        ),
    }]
}

static ADDRESSEE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:宛先[：:]\s*)?(\S{1,30}?(?:様|さま|さん|御中|殿))[、,]?\s*$")
        .expect("valid addressee regex")
});

/// 冒頭5行の宛名（「田中様」「営業部御中」）。メールや手紙で書き換えてはいけない値。
fn addressees(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .take(5)
        .filter_map(|line| {
            ADDRESSEE
                .captures(line)
                .map(|captures| captures[1].to_owned())
        })
        .collect()
}

fn addressee_findings(before: &str, after: &str) -> Vec<DiffFinding> {
    let (old, new) = (addressees(before), addressees(after));
    if old.is_empty() || old == new {
        return Vec::new();
    }
    vec![DiffFinding {
        category: "addressee_changed".to_owned(),
        severity: "warn".to_owned(),
        before_line: Some(1),
        after_line: Some(1),
        before_excerpt: old.join(" / "),
        after_excerpt: new.join(" / "),
        detail: "冒頭の宛名が変わりました。名前・敬称・部署名は原文のとおりに残してください。"
            .to_owned(),
    }]
}
