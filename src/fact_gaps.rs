//! 最終稿の中で、比喩・効果・程度の語だけが書かれ、中身や値が見えない文を候補として示す。
//! 値を補ったり推測したりはしない。候補は一部だけで、候補にない穴もある。

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::text;

#[derive(Debug, Serialize)]
pub struct FactGap {
    /// figure / effect / degree / change
    pub kind: &'static str,
    pub line: usize,
    pub excerpt: String,
    /// 文の中で当たった語。
    pub matched: String,
    /// 足りないかもしれない事実の種類。
    pub need: &'static str,
}

#[derive(Debug, Serialize)]
pub struct FactGapReport {
    /// 対象外のジャンルで候補を出さなかった場合、そのジャンル。
    pub skipped: Option<String>,
    pub count: usize,
    pub by_kind: BTreeMap<&'static str, usize>,
    pub findings: Vec<FactGap>,
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid fact-gap pattern")
}

/// 判定の順。1つの文では最初に当たった種類だけを返す。
static KINDS: LazyLock<[(&str, &str, Regex); 4]> = LazyLock::new(|| {
    [
        (
            "figure",
            "比喩が指す中身（何が起きたか、何をしたか）",
            regex(
                r"土台(?:づくり|作り)?|礎|基盤づくり|第一歩|大きな一歩|一歩前進|連鎖を断ち切|断ち切り|道を(?:開|拓|ひら)|扉を(?:開|ひら)|(?:鍵|カギ)を握|武器(?:に|と)な|強力な武器|景色が(?:変わ|一変)|新たな(?:地平|ステージ|次元)|次の(?:ステージ|次元)|底上げ|盤石|追い風|起爆剤|羅針盤|架け橋|屋台骨|息を吹き込|命を吹き込|静かに(?:壊れ|失敗|落ち)|黙って(?:無視|捨て|スキップ|破棄|止ま)",
            ),
        ),
        (
            "effect",
            "何がどう変わるか",
            regex(
                r"(?:信頼性|安定性|品質|性能|パフォーマンス|使い勝手|利便性|生産性|効率|保守性|可読性|開発体験|体験|満足度|安全性|堅牢性)(?:が|を)(?:向上|高ま|高め|改善|上が|上げ|支え|底上げ|強化)|(?:混乱|影響|リスク|負担|手間|ダウンタイム|ばらつき)(?:を|が)(?:最小限|最小|低減|軽減|抑え|減ら)|(?:改善|最適化|効率化|強化|刷新)(?:しました|します|を行いました|を行います|を実施しました|を図りました|を図ります)|シームレス(?:に|な)|(?:柔軟な|より良い|最適な)(?:選択肢|運用|対応|体験)",
            ),
        ),
        (
            "degree",
            "程度の値（何がどれだけ）",
            regex(
                r"大幅(?:に|な|の)|劇的(?:に|な)|飛躍的(?:に|な)|格段(?:に|の)|著し(?:く|い)|圧倒的(?:に|な)|桁違い|段違い|大きく(?:改善|向上|短縮|削減|減|増|変わ|変え|速|下が|上が|低下|上昇|低減|前進|進歩)|大きな(?:改善|効果|成果|前進|進歩|変化|向上)",
            ),
        ),
        (
            "change",
            "変わる前と後の値",
            // 和語の変化は丁寧形だけにし、小説や随筆の「短くなった」「首を伸ばした」を拾わない。
            regex(
                r"(?:短く|長く|速く|早く|遅く|軽く|重く|多く|少なく|高く|低く|広く|狭く|細かく|厳しく|緩く|ゆるく)(?:し(?:ました|ます|ています|ていました)|なり(?:ました|ます)|なって(?:います|いました))|(?:増や|減ら|延ば|伸ば|縮め|引き上げ|引き下げ)(?:し(?:ました|ます|ています)|ます|ました)|(?:短縮|削減|増加|減少|向上|高速化|軽量化|低減|縮小|拡大|延長|増強|緩和)(?:し(?:ました|ます|ています)|され(?:ました|ます|ています)|でき(?:ました|ます)|した$|する$)",
            ),
        ),
    ]
});

/// 推量・確度・ぼかし・意図した省略・抱負・仮定。これがある文は埋めるよう促さない。
static RESERVED: LazyLock<Regex> = LazyLock::new(|| {
    regex(concat!(
        r"かもしれ|かも知れ|可能性|予定|未定(?:[^義]|$)|未確認|確認済み|確定|暫定|検討中|検討して|調査中|見込み|はず(?:[^れさしすせ]|$)",
        r"|でしょう|だろう|と思|ようです|らしい|おそれ|恐れ|と考えられ|と見られ|たぶん|多分|おそらく|恐らく|きっと|気がする|記憶では|方向で|まだ決まって|確かめていない",
        r"|れば|なら(?:[、。]|$)|たら(?:[、。]|$)|もし|仮に|場合|とすれば|としたら|ならば",
        r"|一部の|複数|いくつか|数(?:社|時間|分|秒|日|週間|か月|年|倍|割|件|回|人|台|十|百|千|万)|近日|後日|別途|ある取引先|関係者",
        r"|省きます|省略|伏せ|社外秘|非公開|控え|今後|これからも|引き続き|まいります|ていきます|ていきたい|続けます|に向けて",
    ))
});

/// 当たった語のすぐ後が打ち消し（大きく変わりません、一度に大きく変えず）。
static NEGATED_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"^[^、。，！？!?]{0,6}?(?:ない|ません|ず(?:[^つっ]|$)|なかっ|(?:する|される|した|している)こと(?:は|が)(?:ない|ありません))",
    )
});

static KANJI_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"[〇一二三四五六七八九十百千万]+(?:倍|割|分|秒|時間|日|件|回|人|個|%|％)")
});

fn has_number(sentence: &str) -> bool {
    sentence
        .chars()
        .any(|ch| ch.is_ascii_digit() || ('０'..='９').contains(&ch))
        || KANJI_NUMBER.is_match(sentence)
}

fn is_question(sentence: &text::Sentence) -> bool {
    matches!(sentence.end_mark, Some('?' | '？'))
        || sentence
            .text
            .trim_end_matches(['」', '』', '）', ')'])
            .ends_with('か')
}

/// 正規表現で書けない前後の条件。「道を開ける」「基礎」「礎石」は比喩の言い回しではない。
fn literal_context(body: &str, start: usize, end: usize) -> bool {
    let matched = &body[start..end];
    let next = body[end..].chars().next();
    let previous = body[..start].chars().next_back();
    ((matched.starts_with("道を") || matched.starts_with("扉を")) && next == Some('け'))
        || (matched == "礎" && (previous == Some('基') || next == Some('石')))
}

fn first_gap(body: &str) -> Option<(&'static str, &'static str, String)> {
    KINDS.iter().find_map(|(kind, need, pattern)| {
        pattern
            .find_iter(body)
            .find(|found| {
                !literal_context(body, found.start(), found.end())
                    && !NEGATED_TAIL.is_match(&body[found.end()..])
                    // 「本体より短く」のように比べる元があれば、値がなくても変化は読める。
                    && !(*kind == "change" && body[..found.start()].contains("より"))
            })
            .map(|found| (*kind, *need, found.as_str().to_owned()))
    })
}

// nwiizo-coding-style: yomiyasu の fact_gaps と同じ4種の語と除外条件を、文単位の正規表現で照合する;
// 意味の判定や値の補完はせず、引用・コード・見出し・表・数値のある文は対象外にする。
pub fn analyze(raw: &str, genre: Option<&str>) -> FactGapReport {
    if genre == Some("essay") {
        return FactGapReport {
            skipped: Some("essay".to_owned()),
            count: 0,
            by_kind: BTreeMap::new(),
            findings: Vec::new(),
        };
    }
    let visible = text::revision_text(raw);
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let sentences = text::sentences_with_raw(&visible, raw)
        .into_iter()
        .filter(|sentence| {
            let line = raw_lines.get(sentence.line - 1).copied().unwrap_or("");
            let trimmed = line.trim_start();
            !text::is_heading(line) && !trimmed.starts_with('|')
        })
        .collect::<Vec<_>>();
    let mut findings = Vec::new();
    for (index, sentence) in sentences.iter().enumerate() {
        let body = sentence.text.as_str();
        // 引用やインラインコードの中の言い回しは、書き手の主張ではない。
        if body.contains(['「', '」', '『', '』'])
            || sentence.raw_text.contains('`')
            || has_number(body)
            || is_question(sentence)
            || RESERVED.is_match(body)
        {
            continue;
        }
        // 直後の文に値があれば、そこで中身を示している。
        if sentences.get(index + 1).is_some_and(|next| {
            next.line.saturating_sub(sentence.line) <= 1 && has_number(&next.text)
        }) {
            continue;
        }
        if let Some((kind, need, matched)) = first_gap(body) {
            findings.push(FactGap {
                kind,
                line: sentence.line,
                excerpt: body.chars().take(160).collect(),
                matched,
                need,
            });
        }
    }
    let mut by_kind = BTreeMap::new();
    for finding in &findings {
        *by_kind.entry(finding.kind).or_insert(0) += 1;
    }
    FactGapReport {
        skipped: None,
        count: findings.len(),
        by_kind,
        findings,
    }
}
