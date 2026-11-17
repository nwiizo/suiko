//! 語の族: 技術の対象や処理を、色・出荷・場所・「効く」「潰す」などの現場の言い回しで表す箇所
//! （`technical_jargon_metaphor`）。本来の表示色・配送・建物の入口は除く。

use crate::morphology::Morpheme;

use super::Finding;
use super::morph::{
    TokenizedSentence, affirmative_predicate, contains_any, position_any, punctuation_between,
};

const TECHNICAL_STATUS_NOUNS: &[&str] = &[
    "テスト",
    "テスト結果",
    "検査結果",
    "CI",
    "ビルド",
    "パイプライン",
];

const DISPLAY_NOUNS: &[&str] = &[
    "表示",
    "アイコン",
    "ランプ",
    "バッジ",
    "画面",
    "背景",
    "文字",
    "色",
    "線",
];

const SOFTWARE_NOUNS: &[&str] = &[
    "コード",
    "機能",
    "変更",
    "修正",
    "ソフトウェア",
    "API",
    "実装",
];

const PHYSICAL_SHIPMENT_NOUNS: &[&str] = &[
    "工場", "倉庫", "在庫", "配送", "端末", "機器", "製品", "検品",
];

const TECHNICAL_WORDING_NOUNS: &[&str] = &[
    "コード",
    "機能",
    "実装",
    "設定",
    "データ",
    "入力",
    "出力",
    "処理",
    "ログ",
    "API",
    "テスト",
    "ビルド",
    "キャッシュ",
    "クエリ",
    // 品質特性・状態も、気づけない劣化の主語として同じ比喩に現れる。
    "整合性",
    "互換性",
    "一貫性",
    "アクセシビリティ",
    "性能",
    "状態",
    "スキーマ",
    "レイアウト",
];

const DEFECT_NOUNS: &[&str] = &[
    "バグ",
    "不具合",
    "エラー",
    "警告",
    "脆弱性",
    "課題",
    "問題",
    "問題点",
    "懸念",
    "懸念点",
    "リスク",
    "指摘",
];

/// テスト結果を色で、ソフトウェアの公開を物流語で表す技術現場の言い回し。
/// 単語だけでは本来の表示色や物理配送も拾うため、同じ節の対象語と状態語を
/// 組み合わせ、表示・配送を示す名詞がある文は除外する。
pub(super) fn technical_jargon_metaphor_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        let mut starts = Vec::new();
        for index in 0..sentence.tokens.len() {
            if let Some((start, detail)) = technical_wording_start(sentence, index) {
                starts.push(start);
                findings.push(sentence.info_finding(
                    raw_lines,
                    start..sentence.tokens[index].byte_end,
                    "technical_jargon_metaphor",
                    detail,
                ));
            }
        }
        // 「地味に効いてくる」を既に指した文では、同じ箇所を分裂文として重ねない。
        if let Some((start, end)) = effect_cleft_span(&sentence.tokens)
            && !starts.iter().any(|known| (start..end).contains(known))
        {
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "technical_jargon_metaphor",
                "「効いてくるのが／のは」で、後に続く内容の効果を予告する言い回し。何がどの条件で改善するのかを、主語と述語で直接書けるか確認してください",
            ));
        }
        if let Some((start, end)) = defect_crush_span(&sentence.tokens) {
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "technical_jargon_metaphor",
                "不具合や課題への対応を「潰す」で表す技術現場の言い回し。直す、取り除く、再発を防ぐなど、実際に行った作業を書けるか確認してください",
            ));
        }
        // 既に指した箇所（地味に効く、効いてくるのが）と重ねない。
        if let Some((start, end)) = subject_effect_span(&sentence.tokens)
            && !starts.iter().any(|known| (start..end).contains(known))
            && effect_cleft_span(&sentence.tokens).is_none()
        {
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "technical_jargon_metaphor",
                "仕組みや工夫の効果を「効く」で表しています。何がどの条件でどう変わったか（時間・件数・失敗の減り方など）を書けるか確認してください。薬や冷暖房のように文字どおり効く場合は残してください",
            ));
        }
        if let Some((start, end)) = technical_place_span(&sentence.tokens) {
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "technical_jargon_metaphor",
                "処理や仕組みの始まりを「入口」「窓口」などの場所で表しています。どの関数・画面・処理から始まるのかを、決まった呼び名で書けるか確認してください。建物や組織の実際の入口・窓口なら残してください",
            ));
        }
        if !contains_any(&sentence.tokens, DISPLAY_NOUNS)
            && let Some((start, end)) = color_status_span(&sentence.tokens)
        {
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "technical_jargon_metaphor",
                "テストや検査の状態を色で表す技術現場の言い回し。何が通ったか、成功したかを直接書けるか確認してください",
            ));
        }

        if !contains_any(&sentence.tokens, PHYSICAL_SHIPMENT_NOUNS)
            && let Some((start, end)) = software_shipment_span(&sentence.tokens)
        {
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "technical_jargon_metaphor",
                "ソフトウェアの公開を物流語で表す技術現場の言い回し。公開、配布、リリースなど具体的な動作を書けるか確認してください",
            ));
        }
    }
    findings
}

/// 活用は基本形で、短い修飾句は表層とtoken境界で照合する。
/// 「静か/に」の「に」は助動詞なので、格助詞だけに絞ると見落とす。
// nwiizo-coding-style: 対象名詞と隣接する修飾句を限定する;
// 見落としの実例が集まったら、同じ形の反例を加えて対象を広げる。
fn technical_wording_start(
    sentence: &TokenizedSentence,
    index: usize,
) -> Option<(usize, &'static str)> {
    let token = &sentence.tokens[index];
    let (prefixes, needs_context, detail): (&[&str], bool, &str) = match token.dictionary_form() {
        "壊れる" | "失敗" | "捨てる" | "無視" | "失う" | "消える" | "スキップ" | "沈黙"
        | "破棄" | "止まる" => {
            if matches!(
                token.dictionary_form(),
                "失敗" | "無視" | "スキップ" | "沈黙" | "破棄"
            ) && !sentence
                .tokens
                .get(index + 1)
                .is_some_and(|next| next.dictionary_form() == "する")
            {
                return None;
            }
            (
                &["静かに", "黙って"],
                true,
                "技術的な失敗・破棄・消失を「静かに／黙って」で表しています。気づかなかったことと、通知やエラーを出さない仕様を区別し、資料から分かる動作を書けるか確認してください。仕組みや原因は推測で足さないでください",
            )
        }
        "効く" => (
            &["地味に"],
            true,
            "技術的な効果を「地味に効く」で表す言い回し。何が改善するか、条件や観測した結果を書けるか確認してください",
        ),
        "溶かす" => (
            &["時間を"],
            false,
            "時間を失った感覚を表す比喩。本人の後悔や実感が伝わるなら残せます。説明する作業が読み取れない場合だけ、原文にある内容で言い換えを検討してください",
        ),
        "倒す" => (
            &["安全側に", "保守側に"],
            true,
            "判断を「側に倒す」で表す言い回し。何を優先し、どの設定や動作を選ぶか明記できるか確認してください",
        ),
        _ => return None,
    };
    let before = &sentence.text[..token.byte_start];
    let prefix = prefixes.iter().find(|prefix| before.ends_with(**prefix))?;
    let predicate_end = if token.pos(0) == "名詞"
        && sentence
            .tokens
            .get(index + 1)
            .is_some_and(|next| next.dictionary_form() == "する")
    {
        index + 1
    } else {
        index
    };
    if !affirmative_predicate(&sentence.tokens.iter().collect::<Vec<_>>(), predicate_end) {
        return None;
    }
    let start = token.byte_start - prefix.len();
    let prefix_index = sentence.tokens[..index]
        .iter()
        .position(|part| part.byte_start == start)?;
    if needs_context {
        // 直近の「名詞+は/が/を/も」を確認し、別の人物・対象へ文脈を持ち越さない。
        // 強調の`**`・`_`は読み飛ばし、「設定が**静かに壊れる**」も同じ文脈として扱う。
        let plain = sentence.tokens[..prefix_index]
            .iter()
            .filter(|token| !token.surface.chars().all(|ch| matches!(ch, '*' | '_')))
            .collect::<Vec<_>>();
        let argument = plain
            .windows(2)
            .rev()
            .take(12)
            .take_while(|pair| !matches!(pair[1].pos(0), "記号" | "補助記号" | "空白" | "動詞"))
            .find(|pair| {
                pair[1].pos(0) == "助詞"
                    && matches!(pair[1].surface.as_str(), "は" | "が" | "を" | "も")
            });
        if !argument.is_some_and(|pair| {
            pair[0].pos(0) == "名詞" && TECHNICAL_WORDING_NOUNS.contains(&pair[0].dictionary_form())
        }) {
            return None;
        }
    }
    Some((start, detail))
}

fn color_status_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let subject = position_any(tokens, TECHNICAL_STATUS_NOUNS)?;
    let color = tokens.iter().position(|token| {
        matches!(token.dictionary_form(), "緑" | "グリーン")
            && matches!(token.pos(0), "名詞" | "形状詞")
    })?;
    if subject >= color || punctuation_between(tokens, subject, color) {
        return None;
    }
    let following = &tokens[color + 1..];
    let state = following.iter().take(4).position(|token| {
        matches!(
            token.dictionary_form(),
            "だ" | "です" | "なる" | "戻す" | "保つ" | "まま"
        ) || token.surface == "で"
    })? + color
        + 1;
    if punctuation_between(tokens, color, state) {
        return None;
    }
    Some((tokens[subject].byte_start, tokens[state].byte_end))
}

fn software_shipment_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let shipment = tokens
        .iter()
        .position(|token| token.dictionary_form() == "出荷")?;
    let software = position_any(&tokens[..shipment], SOFTWARE_NOUNS)
        .filter(|index| !punctuation_between(tokens, *index, shipment));
    let scoped_state = tokens[..shipment]
        .iter()
        .enumerate()
        .position(|(index, token)| {
            token.dictionary_form() == "状態"
                && index > 0
                && tokens[index - 1].surface == "どの"
                && !punctuation_between(tokens, index, shipment)
        });
    let action = tokens[shipment + 1..]
        .iter()
        .position(|token| token.pos(0) == "動詞")
        .map(|index| index + shipment + 1);
    if !action.is_some_and(|index| matches!(tokens[index].dictionary_form(), "する" | "止める"))
    {
        return None;
    }
    let stop = action.filter(|index| tokens[*index].dictionary_form() == "止める");
    let decide = tokens
        .iter()
        .position(|token| token.dictionary_form() == "決める");
    let context =
        software.or_else(|| scoped_state.filter(|_| stop.is_some() && decide.is_some()))?;
    let end = action.expect("shipment action exists");
    Some((
        tokens[context.min(shipment)].byte_start,
        tokens[end].byte_end,
    ))
}

/// 効果を「効く」で言う抽象的な主語。
const EFFECT_SUBJECT_NOUNS: &[&str] = &[
    "工夫",
    "仕組み",
    "ルール",
    "対策",
    "改善",
    "習慣",
    "区別",
    "設計",
    "施策",
    "手順",
    "運用",
    "見直し",
    "整理",
    "準備",
    "変更",
    "方針",
    "取り組み",
    "仕掛け",
    "自動化",
];

/// 文字どおりに効くもの（薬・冷暖房・調味料・ブレーキ）。
const LITERAL_EFFECT_NOUNS: &[&str] = &[
    "薬",
    "薬剤",
    "麻酔",
    "冷房",
    "暖房",
    "エアコン",
    "空調",
    "調味料",
    "塩",
    "胡椒",
    "スパイス",
    "わさび",
    "ワサビ",
    "山葵",
    "ブレーキ",
    "肥料",
    "殺虫剤",
];

/// 「ルールが、効いた」「この区別が効いてくる」のように、抽象的な主語＋が／は／も＋効く。
/// 否定形（効かない）は、効果を言い立てる言い回しではないため除く。
fn subject_effect_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    if contains_any(tokens, LITERAL_EFFECT_NOUNS) {
        return None;
    }
    let verb = tokens
        .iter()
        .position(|token| token.pos(0) == "動詞" && token.dictionary_form() == "効く")?;
    if tokens[verb + 1..].iter().take(3).any(|token| {
        token.pos(0) == "助動詞" && matches!(token.dictionary_form(), "ない" | "ぬ" | "ず")
    }) {
        return None;
    }
    // 主語の助詞から動詞までは、読点・副詞・「〜に」だけを許す。
    let particle = tokens[..verb].iter().rposition(|token| {
        !matches!(token.pos(0), "補助記号" | "空白" | "副詞" | "形状詞")
            && !(token.pos(0) == "助動詞" && token.surface == "に")
    })?;
    let marker = &tokens[particle];
    let subject = tokens.get(particle.checked_sub(1)?)?;
    let abstract_subject = EFFECT_SUBJECT_NOUNS
        .iter()
        .any(|noun| subject.dictionary_form() == *noun || subject.surface.ends_with(noun));
    (marker.pos(0) == "助詞"
        && matches!(marker.surface.as_str(), "が" | "は" | "も")
        && subject.pos(0) == "名詞"
        && abstract_subject)
        .then_some((subject.byte_start, tokens[verb].byte_end))
}

/// 処理の始まりを指す場所の名詞。
const PLACE_NOUNS: &[&str] = &["入口", "入り口", "窓口", "受け口", "玄関"];

const TECHNICAL_CONTEXT_NOUNS: &[&str] = &[
    "コード",
    "処理",
    "関数",
    "API",
    "データ",
    "計算",
    "システム",
    "サービス",
    "モジュール",
    "リクエスト",
    "実装",
    "機能",
    "ログ",
    "通信",
    "サーバー",
    "クラス",
    "メソッド",
    "イベント",
    "ジョブ",
    "パイプライン",
    "エンドポイント",
    "ライブラリ",
    "プログラム",
    "呼び出し",
    "連携",
    "ソフトウェア",
    "アプリ",
    "アプリケーション",
    "インターフェース",
];

/// 建物・駅・店など、入口・玄関を実物として使う文脈。
const PHYSICAL_PLACE_CONTEXT: &[&str] = &[
    "駅",
    "建物",
    "ビル",
    "店",
    "店舗",
    "家",
    "部屋",
    "扉",
    "ドア",
    "改札",
    "傘",
    "校舎",
    "教室",
    "図書館",
    "病院",
    "会場",
    "庁舎",
];

/// 組織の窓口（問い合わせ窓口・相談窓口）。
const ORGANIZATION_WINDOW_PREFIXES: &[&str] = &[
    "問い合わせ",
    "問合せ",
    "相談",
    "質問",
    "受付",
    "担当",
    "総合",
    "申請",
    "セキュリティ",
];

/// 技術の語がある文で、「請求計算の入口」「操作を入口に」のように処理の始まりを場所で言う範囲。
fn technical_place_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let technical = |token: &Morpheme| {
        token.pos(0) == "名詞"
            && TECHNICAL_CONTEXT_NOUNS
                .iter()
                .any(|noun| token.dictionary_form() == *noun || token.surface.ends_with(noun))
    };
    if !tokens.iter().any(technical) || contains_any(tokens, PHYSICAL_PLACE_CONTEXT) {
        return None;
    }
    tokens.iter().enumerate().find_map(|(index, token)| {
        if token.pos(0) != "名詞" || !PLACE_NOUNS.contains(&token.dictionary_form()) {
            return None;
        }
        let previous = index.checked_sub(1).map(|before| &tokens[before]);
        if previous.is_some_and(|before| {
            ORGANIZATION_WINDOW_PREFIXES.contains(&before.dictionary_form())
                || ORGANIZATION_WINDOW_PREFIXES.contains(&before.surface.as_str())
        }) {
            return None;
        }
        let particle = previous?;
        let owner_end = index.checked_sub(2)?;
        let owner = &tokens[owner_end];
        let genitive = particle.surface == "の" && technical(owner);
        let based_on = particle.surface == "を"
            && owner.pos(0) == "名詞"
            && tokens
                .get(index + 1)
                .is_some_and(|next| next.surface == "に");
        if !genitive && !based_on {
            return None;
        }
        // 「請求計算の入口」のように、名詞の連なりの頭から示す。
        let start = tokens[..=owner_end]
            .iter()
            .rposition(|token| !matches!(token.pos(0), "名詞" | "接頭辞"))
            .map_or(0, |before| before + 1);
        Some((tokens[start].byte_start, token.byte_end))
    })
}

/// 効く＋て＋くる（＋た）＋準体の「の」＋が／は。効く対象を後置して予告する分裂文に限り、
/// 「薬が効いてくるのは」のように同じ節で主語を先に示す文は除く。
fn effect_cleft_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let verb = tokens
        .iter()
        .position(|token| token.pos(0) == "動詞" && token.dictionary_form() == "効く")?;
    let mut next = verb + 1;
    if tokens.get(next)?.surface != "て" || tokens.get(next + 1)?.dictionary_form() != "くる" {
        return None;
    }
    next += 2;
    if tokens
        .get(next)
        .is_some_and(|token| token.pos(0) == "助動詞" && token.dictionary_form() == "た")
    {
        next += 1;
    }
    let nominalizer = tokens.get(next)?;
    let topic = tokens.get(next + 1)?;
    if nominalizer.surface != "の"
        || nominalizer.pos(1) != "準体助詞"
        || !matches!(topic.surface.as_str(), "が" | "は")
        || topic.pos(0) != "助詞"
    {
        return None;
    }
    let clause = tokens[..verb]
        .iter()
        .rposition(|token| matches!(token.pos(0), "記号" | "補助記号"))
        .map_or(0, |index| index + 1);
    if tokens[clause..verb].windows(2).any(|pair| {
        matches!(pair[0].pos(0), "名詞" | "代名詞")
            && pair[1].pos(0) == "助詞"
            && matches!(pair[1].surface.as_str(), "が" | "は")
    }) {
        return None;
    }
    // 「地味に」「じわじわ」等の直前の修飾は指摘範囲へ含め、読み直す単位を揃える。
    let start = match verb.checked_sub(2).map(|index| &tokens[index..verb]) {
        Some([manner, copula])
            if manner.pos(0) == "形状詞" && copula.surface == "に" && clause + 2 <= verb =>
        {
            verb - 2
        }
        _ => verb,
    };
    Some((tokens[start].byte_start, topic.byte_end))
}

/// 不具合を表す名詞＋を＋（数量・ずつ等）＋潰す。「時間を潰す」「箱を潰す」は対象名詞で除く。
fn defect_crush_span(tokens: &[Morpheme]) -> Option<(usize, usize)> {
    let verb = tokens
        .iter()
        .position(|token| token.pos(0) == "動詞" && token.dictionary_form() == "潰す")?;
    let object = tokens[..verb]
        .iter()
        .rposition(|token| token.surface == "を" && token.pos(1) == "格助詞")?;
    if verb - object > 5
        || tokens[object + 1..verb]
            .iter()
            .any(|token| matches!(token.pos(0), "記号" | "補助記号" | "動詞" | "形容詞"))
    {
        return None;
    }
    // 複合名詞は主要部（末尾）か全体で照合する。「型エラー」「懸念＋点」を拾い、「バグ報告」は拾わない。
    let head = object.checked_sub(1)?;
    let first = tokens[..object]
        .iter()
        .rposition(|token| token.pos(0) != "名詞" || token.pos(1) == "数詞")
        .map_or(0, |index| index + 1);
    if first > head {
        return None;
    }
    let compound = tokens[first..object]
        .iter()
        .map(|token| token.surface.as_str())
        .collect::<String>();
    if !DEFECT_NOUNS.contains(&tokens[head].dictionary_form())
        && !DEFECT_NOUNS.contains(&compound.as_str())
    {
        return None;
    }
    Some((tokens[first].byte_start, tokens[verb].byte_end))
}
