//! 文末の働き（依頼・許可・可能・推量など）の判定。

use std::sync::LazyLock;

use regex::Regex;

use super::units::{Unit, tail_start};

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
pub(super) fn ending_tail(text: &str) -> &str {
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

pub(super) fn ending_kind(unit: &Unit) -> &'static str {
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
