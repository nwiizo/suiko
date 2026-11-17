//! 変更前後の単位の対応づけと、変更された領域の切り出し。

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::PAIR_SIMILARITY;
use super::units::{Unit, similarity};

const LCS_CELLS: usize = 40_000;

/// 空白を除いて一致する単位を、文書内で一度しか現れない単位を錨にして前から対応づける。
/// 錨のない範囲は小さければ最長共通部分列で、大きければ両端からの一致だけで対応づける。
pub(super) fn align(before: &[Unit], after: &[Unit]) -> Vec<(usize, usize)> {
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

pub(super) struct Region {
    pub(super) before: Vec<usize>,
    pub(super) after: Vec<usize>,
}

/// 一致しない単位を領域にまとめる。前後の両方で一致しないまま残った同じキーは、
/// 位置が変わっただけの単位として領域から外す。
pub(super) fn regions(
    before: &[Unit],
    after: &[Unit],
    pairs: &[(usize, usize)],
) -> (Vec<Region>, usize) {
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

/// 変更後の各文に、最も似た変更前の文を割り当てる。1文が1文に置き換わった領域は無条件に対にする。
pub(super) fn pair_units(before: &[&Unit], after: &[&Unit]) -> Vec<Option<usize>> {
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

pub(super) fn coverage(unit: &Unit, block: &[&Unit]) -> f64 {
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
