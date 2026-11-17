//! 文体と語調: 気持ちの語、約束、敬体と常体、漢字と仮名、強調、宛名。

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use super::endings::ending_tail;
use super::modality::phrase_at;
use super::units::{Unit, UnitKind};
use super::{DiffFinding, finding, join_items};

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
        r"再発防止に努め|努めます|努めてまいります|取り組んでまいります|お詫び|",
        r"(?:保証|担保)(?:します|する|され|いたし|でき)",
    ))
    .expect("valid promise regex")
});
// 約束のうち、原文にない保証や再発防止の誓いは事実の主張を変えるので warn にする。
static GUARANTEE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:お約束|約束します|二度と|再発防止に努め|(?:保証|担保).+)$")
        .expect("valid guarantee regex")
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

pub(super) fn feeling_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
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

pub(super) fn promise_findings(before: &[&Unit], after: &[&Unit], findings: &mut Vec<DiffFinding>) {
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
    let guaranteed = added.iter().any(|item| GUARANTEE.is_match(item));
    findings.push(finding(
        "promise_added",
        if guaranteed { "warn" } else { "info" },
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

pub(super) fn register_findings(before: &[Unit], after: &[Unit]) -> Vec<DiffFinding> {
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

fn is_kanji(ch: char) -> bool {
    matches!(ch, '一'..='龥' | '々' | '〆')
}

/// 1対1で対応した文の、読点・漢字の割合・敬体と常体・見出しの文言の変化。
pub(super) fn style_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
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
pub(super) fn tone_findings(before: &[Unit], after: &[Unit]) -> Vec<DiffFinding> {
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

/// 冒頭5行の宛名（「田中様」「営業部御中」）と、その行番号（1始まり）。
/// メールや手紙で書き換えてはいけない値。
fn addressees(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(5)
        .filter_map(|(index, line)| {
            ADDRESSEE
                .captures(line)
                .map(|captures| (index + 1, captures[1].to_owned()))
        })
        // 「仕様」「同様」のように「様」で終わる普通の語だけの行（見出しなど）は宛名にしない。
        .filter(|(_, name)| !NOT_HONORIFIC_SAMA.iter().any(|word| name.ends_with(word)))
        .collect()
}

const NOT_HONORIFIC_SAMA: &[&str] = &[
    "仕様", "同様", "模様", "多様", "異様", "一様", "有様", "両様", "文様", "紋様",
];

pub(super) fn addressee_findings(before: &str, after: &str) -> Vec<DiffFinding> {
    let (old, new) = (addressees(before), addressees(after));
    fn names(items: &[(usize, String)]) -> Vec<&str> {
        items.iter().map(|(_, name)| name.as_str()).collect()
    }
    if names(&old) == names(&new) {
        return Vec::new();
    }
    let detail = if old.is_empty() {
        "改稿で冒頭に宛名が加わりました。原文にない宛先・敬称を書き足していないか、宛先が正しいか確認してください。"
    } else {
        "冒頭の宛名が変わりました。名前・敬称・部署名は原文のとおりに残してください。"
    };
    vec![DiffFinding {
        category: "addressee_changed".to_owned(),
        severity: "warn".to_owned(),
        before_line: old.first().map(|(line, _)| *line),
        after_line: new.first().map(|(line, _)| *line),
        before_excerpt: names(&old).join(" / "),
        after_excerpt: names(&new).join(" / "),
        detail: detail.to_owned(),
    }]
}
