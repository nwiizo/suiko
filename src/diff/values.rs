//! 変えてはいけない値の照合: 数値、識別子、引用、出典。

use std::sync::LazyLock;

use regex::Regex;

use super::units::{URL, Unit};
use super::{
    DiffFinding, DocumentValues, block_raw, block_text, distinct, item_change, multiset_diff,
};
use crate::text::inline_code_ranges;

// 数値。桁区切りと全角・半角の違いは同じ値として扱い、約・最大・以上などの修飾を含めて比べる。
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:[<>＜＞≦≧≤≥]=?[ \t]*)?",
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
// 曜日は値として比べる（「金曜」→「土曜」）。「金曜日」「金曜」「3日(金)」は同じ値。
static WEEKDAY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([月火水木金土日])曜|[0-9０-９]日[（(]([月火水木金土日])(?:・祝)?[）)]")
        .expect("valid weekday regex")
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
            '＜' => '<',
            '＞' => '>',
            '≦' => '≤',
            '≧' => '≥',
            other => other,
        })
        .collect::<String>()
        .replace("<=", "≤")
        .replace(">=", "≥")
}

/// 期間を表す「間」を外した値（「30日間」→「30日」）。
fn without_period(value: &str) -> String {
    [("日間", "日"), ("年間", "年"), ("週間", "週")]
        .iter()
        .find_map(|(long, short)| {
            value
                .strip_suffix(long)
                .map(|head| format!("{head}{short}"))
        })
        .unwrap_or_else(|| value.to_owned())
}

pub(crate) fn numbers(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for found_match in NUMBER.find_iter(text) {
        let head = &text[..found_match.start()];
        // 「p<0.05」の「p」のように、比較の記号の前の英字は識別子の一部ではない。
        let starts_with_sign = found_match
            .as_str()
            .starts_with(['<', '>', '＜', '＞', '≦', '≧', '≤', '≥']);
        let preceded_by_ascii = !starts_with_sign
            && head
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
    found.extend(WEEKDAY.captures_iter(text).filter_map(|captures| {
        captures
            .get(1)
            .or_else(|| captures.get(2))
            .map(|day| format!("{}曜", day.as_str()))
    }));
    found
}

pub(super) fn number_findings(
    before: &[&Unit],
    after: &[&Unit],
    document_values: &DocumentValues,
    findings: &mut Vec<DiffFinding>,
) {
    let (lost, added) = multiset_diff(
        &distinct(numbers(&block_text(before))),
        &distinct(numbers(&block_text(after))),
    );
    let lost = lost
        .into_iter()
        .filter(|value| !document_values.numbers_after.contains(value))
        .collect::<Vec<_>>();
    let added = added
        .into_iter()
        .filter(|value| !document_values.numbers_before.contains(value))
        .collect::<Vec<_>>();
    // 「30日」→「30日間」のように「間」の有無だけが違う場合は、同じ期間の言い換えが多いので info にする。
    // 「3日」は日付とも読めるため、同じ値とはみなさない。
    let periods = |values: &[String]| {
        let mut keys = values
            .iter()
            .map(|value| without_period(value))
            .collect::<Vec<_>>();
        keys.sort();
        keys
    };
    let period_only = !lost.is_empty() && periods(&lost) == periods(&added);
    findings.extend(item_change(
        "number_changed",
        if period_only { "info" } else { "warn" },
        before,
        after,
        &lost,
        &added,
        |lost, added| {
            let note = if period_only {
                "期間を表す「間」の有無だけが違います。日付と期間を読み違えないか確認してください。"
            } else {
                "約・最大・以上などの修飾を含めて、値と範囲が原文と同じか確認してください。"
            };
            format!("数値や単位の表現が変わりました。消えた値: {lost} / 増えた値: {added}。{note}")
        },
    ));
}

// 識別子。コード片、URL、英数字の語を大文字小文字を区別して比べる。
static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z_][A-Za-z0-9_]*(?:[.\-/:+][A-Za-z0-9_]+)*").expect("valid identifier regex")
});

pub(super) fn identifiers(raw: &str) -> Vec<String> {
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

pub(super) fn identifier_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
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

pub(super) fn quotation_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
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

pub(super) fn citation_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
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
