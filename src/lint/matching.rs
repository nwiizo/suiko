//! 改稿前後の指摘を対応づける。compare・--baseline・check が同じ規則で継続を判定する。
//!
//! 本文があるときは行を対応づけ（両方に一度だけ現れる行を目印にし、その間の変更行を
//! 文字の近さで組む）、強い根拠から順に指摘を組む。根拠の弱い対応は継続にしない。

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

/// 文書全体の統計で、抜粋が行に結び付かないカテゴリ。種類だけで対応づける。
const DOCUMENT_LEVEL: &[&str] = &[
    "antithesis_repetition",
    "repeated_distinction",
    "repeated_em_dash",
    "low_burstiness",
    "high_length_autocorrelation",
    "low_sentence_variance",
    "uniform_paragraph_structure",
    "nominal_ending",
    "low_lexical_diversity_ttr",
    "low_lexical_diversity_mtld",
];

/// 変更行どうしを同じ文の書き換えとみなす文字bigramの一致度（Dice係数）の下限。
const EDITED_LINE_SIMILARITY: f64 = 0.5;
/// 抜粋の先頭だけで照合するときの文字数（本文がないbaseline用）。
const EXCERPT_PREFIX_CHARS: usize = 20;

/// 照合に使う指摘の要素。Findingとbaseline JSONの両方から作れる。
#[derive(Clone, Copy, Debug)]
pub struct MatchKey<'a> {
    pub line: usize,
    pub category: &'a str,
    pub excerpt: &'a str,
}

impl<'a> From<&'a super::Finding> for MatchKey<'a> {
    fn from(finding: &'a super::Finding) -> Self {
        Self {
            line: finding.line,
            category: &finding.category,
            excerpt: &finding.excerpt,
        }
    }
}

/// 継続と判定した根拠。強い順に並ぶ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchBasis {
    /// 文書全体の統計で、同じ種類が前後に残った。
    Document,
    /// 対応する行で、同じ抜粋が残った。
    SameSentence,
    /// 対応する行が書き換えられ、同じ種類の指摘が残った。
    EditedSentence,
    /// 同じ抜粋が別の位置へ移った。
    Moved,
    /// 本文がなく、抜粋の先頭だけが一致した。
    ExcerptPrefix,
}

impl MatchBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::SameSentence => "same_sentence",
            Self::EditedSentence => "edited_sentence",
            Self::Moved => "moved",
            Self::ExcerptPrefix => "excerpt_prefix",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindingPair {
    pub before: usize,
    pub after: usize,
    pub basis: MatchBasis,
}

#[derive(Debug, Default)]
pub struct FindingMatches {
    /// 継続した指摘（前後の添字と根拠）。後の添字の昇順。
    pub persisting: Vec<FindingPair>,
    /// 改稿後に残らなかった前の指摘の添字。
    pub resolved: Vec<usize>,
    /// 改稿後に新しく出た指摘の添字。
    pub new: Vec<usize>,
}

fn normalized(excerpt: &str) -> String {
    excerpt.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn prefix(excerpt: &str) -> String {
    normalized(excerpt)
        .chars()
        .take(EXCERPT_PREFIX_CHARS)
        .collect()
}

fn bigrams(text: &str) -> Vec<(char, char)> {
    let chars = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<Vec<_>>();
    let mut grams = chars.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>();
    grams.sort_unstable();
    grams
}

/// 文字bigramのDice係数。短い行でも0〜1に収まる。
fn similarity(a: &str, b: &str) -> f64 {
    let (left, right) = (bigrams(a), bigrams(b));
    if left.is_empty() || right.is_empty() {
        return if normalized(a) == normalized(b) {
            1.0
        } else {
            0.0
        };
    }
    dice(&left, &right)
}

/// 整列済みのbigram列どうしのDice係数。
fn dice(left: &[(char, char)], right: &[(char, char)]) -> f64 {
    let (mut i, mut j, mut common) = (0, 0, 0);
    while i < left.len() && j < right.len() {
        match left[i].cmp(&right[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                common += 1;
                i += 1;
                j += 1;
            }
        }
    }
    2.0 * common as f64 / (left.len() + right.len()) as f64
}

/// 対応した行。`exact`は前後で内容が同じ行。行番号は1始まり。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LinePair {
    after: usize,
    exact: bool,
}

/// 最長増加部分列（afterの行番号で増加）を残し、交差する目印を捨てる。
fn increasing(anchors: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![None; anchors.len()];
    for (index, &(_, after)) in anchors.iter().enumerate() {
        let position = tails.partition_point(|&tail| anchors[tail].1 < after);
        if position > 0 {
            previous[index] = Some(tails[position - 1]);
        }
        if position == tails.len() {
            tails.push(index);
        } else {
            tails[position] = index;
        }
    }
    let mut chain = Vec::with_capacity(tails.len());
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        chain.push(anchors[index]);
        cursor = previous[index];
    }
    chain.reverse();
    chain
}

/// 目印の間の行を、同じ内容→近い内容の順に、順序を保って組む。
fn pair_gap(
    before: &[&str],
    after: &[&str],
    (b_start, b_end): (usize, usize),
    (a_start, a_end): (usize, usize),
    map: &mut HashMap<usize, LinePair>,
) {
    let mut next_after = a_start;
    for (b, line) in before.iter().enumerate().take(b_end).skip(b_start) {
        if line.is_empty() {
            continue;
        }
        if let Some(a) = (next_after..a_end).find(|&a| after[a] == *line) {
            map.insert(
                b + 1,
                LinePair {
                    after: a + 1,
                    exact: true,
                },
            );
            next_after = a + 1;
        }
    }
    let mut taken = map
        .values()
        .map(|pair| pair.after - 1)
        .filter(|a| (a_start..a_end).contains(a))
        .collect::<HashSet<_>>();
    let after_grams = (a_start..a_end)
        .map(|a| bigrams(after[a]))
        .collect::<Vec<_>>();
    let mut next_after = a_start;
    for (b, line) in before.iter().enumerate().take(b_end).skip(b_start) {
        if line.is_empty() {
            continue;
        }
        if let Some(pair) = map.get(&(b + 1)) {
            next_after = pair.after;
            continue;
        }
        let grams = bigrams(line);
        let best = (next_after..a_end)
            .filter(|a| !after[*a].is_empty() && !taken.contains(a))
            .map(|a| {
                let score = if grams.is_empty() || after_grams[a - a_start].is_empty() {
                    similarity(line, after[a])
                } else {
                    dice(&grams, &after_grams[a - a_start])
                };
                (a, score)
            })
            .filter(|&(_, score)| score >= EDITED_LINE_SIMILARITY)
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((a, _)) = best {
            map.insert(
                b + 1,
                LinePair {
                    after: a + 1,
                    exact: false,
                },
            );
            taken.insert(a);
            next_after = a + 1;
        }
    }
}

/// 前の行番号から後の行番号への対応。両方に一度だけ現れる行を目印にする。
fn align_lines(before_text: &str, after_text: &str) -> HashMap<usize, LinePair> {
    let before = before_text.lines().map(str::trim).collect::<Vec<_>>();
    let after = after_text.lines().map(str::trim).collect::<Vec<_>>();
    fn count<'t>(lines: &[&'t str]) -> HashMap<&'t str, (usize, usize)> {
        let mut counts = HashMap::new();
        for (index, line) in lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !line.is_empty())
        {
            let entry = counts.entry(*line).or_insert((0, index));
            entry.0 += 1;
        }
        counts
    }
    let (before_counts, after_counts) = (count(&before), count(&after));
    let mut anchors = before_counts
        .iter()
        .filter(|(_, (count, _))| *count == 1)
        .filter_map(|(line, (_, b))| match after_counts.get(line) {
            Some((1, a)) => Some((*b, *a)),
            _ => None,
        })
        .collect::<Vec<_>>();
    anchors.sort_unstable();
    let anchors = increasing(&anchors);
    let mut map = HashMap::new();
    let (mut b_start, mut a_start) = (0, 0);
    for &(b, a) in &anchors {
        pair_gap(&before, &after, (b_start, b), (a_start, a), &mut map);
        map.insert(
            b + 1,
            LinePair {
                after: a + 1,
                exact: true,
            },
        );
        (b_start, a_start) = (b + 1, a + 1);
    }
    pair_gap(
        &before,
        &after,
        (b_start, before.len()),
        (a_start, after.len()),
        &mut map,
    );
    map
}

struct Matcher<'k, 'a> {
    before: &'k [MatchKey<'a>],
    after: &'k [MatchKey<'a>],
    used_before: Vec<bool>,
    used_after: Vec<bool>,
    pairs: Vec<FindingPair>,
}

impl Matcher<'_, '_> {
    /// 未対応の組のうち条件を満たすものを、行の近い順に組む。
    fn pass(&mut self, basis: MatchBasis, accepts: impl Fn(&MatchKey, &MatchKey) -> bool) {
        let mut candidates = Vec::new();
        for (b, old) in self.before.iter().enumerate() {
            if self.used_before[b] {
                continue;
            }
            for (a, new) in self.after.iter().enumerate() {
                if !self.used_after[a] && old.category == new.category && accepts(old, new) {
                    candidates.push((old.line.abs_diff(new.line), b, a));
                }
            }
        }
        candidates.sort_unstable();
        for (_, b, a) in candidates {
            if !self.used_before[b] && !self.used_after[a] {
                self.used_before[b] = true;
                self.used_after[a] = true;
                self.pairs.push(FindingPair {
                    before: b,
                    after: a,
                    basis,
                });
            }
        }
    }
}

/// 前後の指摘を対応づける。`texts`は（改稿前, 改稿後）の本文で、無ければ抜粋だけで照合する。
pub fn match_findings(
    before: &[MatchKey],
    after: &[MatchKey],
    texts: Option<(&str, &str)>,
) -> FindingMatches {
    let mut matcher = Matcher {
        before,
        after,
        used_before: vec![false; before.len()],
        used_after: vec![false; after.len()],
        pairs: Vec::new(),
    };
    matcher.pass(MatchBasis::Document, |old, _| {
        DOCUMENT_LEVEL.contains(&old.category)
    });
    let lines = texts.map(|(before_text, after_text)| align_lines(before_text, after_text));
    let corresponds = |old: &MatchKey, new: &MatchKey| {
        lines
            .as_ref()
            .and_then(|lines| lines.get(&old.line))
            .is_some_and(|pair| pair.after == new.line)
    };
    if lines.is_some() {
        matcher.pass(MatchBasis::SameSentence, |old, new| {
            corresponds(old, new) && normalized(old.excerpt) == normalized(new.excerpt)
        });
        matcher.pass(MatchBasis::EditedSentence, |old, new| corresponds(old, new));
    }
    matcher.pass(MatchBasis::Moved, |old, new| {
        normalized(old.excerpt) == normalized(new.excerpt)
    });
    if lines.is_none() {
        matcher.pass(MatchBasis::ExcerptPrefix, |old, new| {
            prefix(old.excerpt) == prefix(new.excerpt)
        });
    }
    let mut persisting = matcher.pairs;
    persisting.sort_unstable_by_key(|pair| pair.after);
    FindingMatches {
        persisting,
        resolved: (0..before.len())
            .filter(|&b| !matcher.used_before[b])
            .collect(),
        new: (0..after.len())
            .filter(|&a| !matcher.used_after[a])
            .collect(),
    }
}

/// 根拠ごとの件数。出力で継続の内訳を示すために使う。
pub fn basis_counts(matches: &FindingMatches) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for pair in &matches.persisting {
        *counts.entry(pair.basis.as_str()).or_default() += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key<'a>(line: usize, category: &'a str, excerpt: &'a str) -> MatchKey<'a> {
        MatchKey {
            line,
            category,
            excerpt,
        }
    }

    #[test]
    fn edited_sentences_keep_their_findings_after_lines_shift() {
        let before = "見出し\n\nこの施策は大きな違いを作ります。\n最後の文です。\n";
        let after = "見出し\n\n追加した段落です。\n\nこの新しい施策は大きな違いを作ってくれます。\n最後の文です。\n";
        let old = [key(3, "calque_phrase", "違いを作")];
        let new = [key(5, "calque_phrase", "違いを作って")];
        let matches = match_findings(&old, &new, Some((before, after)));
        assert_eq!(
            matches.persisting,
            [FindingPair {
                before: 0,
                after: 0,
                basis: MatchBasis::EditedSentence
            }]
        );
        // 本文がなければ抜粋が違うので、継続にしない。
        let without_text = match_findings(&old, &new, None);
        assert!(without_text.persisting.is_empty());
        assert_eq!(
            (without_text.resolved, without_text.new),
            (vec![0], vec![0])
        );
    }

    #[test]
    fn repeated_expressions_pair_with_the_nearest_corresponding_line() {
        let before = "A。\n本記事では説明します。\nB。\n本記事では説明します。\n";
        let after = "A。\nB。\n本記事では説明します。\n";
        let old = [
            key(2, "forbidden_phrase", "本記事では"),
            key(4, "forbidden_phrase", "本記事では"),
        ];
        let new = [key(3, "forbidden_phrase", "本記事では")];
        let matches = match_findings(&old, &new, Some((before, after)));
        assert_eq!(matches.persisting.len(), 1);
        assert_eq!(matches.persisting[0].before, 1);
        assert_eq!(matches.persisting[0].basis, MatchBasis::SameSentence);
        assert_eq!(matches.resolved, [0]);
    }

    #[test]
    fn unrelated_rewrites_are_resolved_and_new_rather_than_forced() {
        let before = "設定は今日中に終わります。\n";
        let after = "明日の会議で、担当を決めます。\n";
        let old = [key(1, "translationese", "終わります")];
        let new = [key(1, "translationese", "決めます")];
        let matches = match_findings(&old, &new, Some((before, after)));
        assert!(matches.persisting.is_empty());
        assert_eq!((matches.resolved, matches.new), (vec![0], vec![0]));
    }

    #[test]
    fn moved_and_document_level_findings_persist() {
        let before = "一。\n二は大きな違いを作る。\n三。\n";
        let after = "二は大きな違いを作る。\n一。\n三。\n";
        let old = [
            key(2, "calque_phrase", "違いを作"),
            key(1, "low_burstiness", "文書全体"),
        ];
        let new = [
            key(1, "calque_phrase", "違いを作"),
            key(1, "low_burstiness", "文書全体の値"),
        ];
        let matches = match_findings(&old, &new, Some((before, after)));
        let bases = matches
            .persisting
            .iter()
            .map(|pair| pair.basis)
            .collect::<Vec<_>>();
        assert_eq!(bases.len(), 2);
        assert!(bases.contains(&MatchBasis::Document));
        assert!(
            bases.contains(&MatchBasis::SameSentence) || bases.contains(&MatchBasis::Moved),
            "{bases:?}"
        );
    }

    #[test]
    fn baseline_without_text_falls_back_to_the_excerpt_prefix() {
        let old = [key(
            10,
            "translationese",
            "することができる。それは長い説明の続きで",
        )];
        let new = [key(
            12,
            "translationese",
            "することができる。それは長い説明の続きです",
        )];
        let matches = match_findings(&old, &new, None);
        assert_eq!(matches.persisting[0].basis, MatchBasis::ExcerptPrefix);
    }
}
