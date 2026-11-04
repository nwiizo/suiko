//! 助詞の直後から、別の節を越えずに局所的な述語を探す。

use crate::morphology::Morpheme;

pub(super) fn active_predicate(tokens: &[&Morpheme], predicate: usize) -> bool {
    super::morph::affirmative_predicate(tokens, predicate)
        && !tokens[predicate + 1..]
            .iter()
            .take_while(|token| token.pos(0) == "助動詞")
            .any(|token| {
                matches!(
                    token.dictionary_form(),
                    "れる" | "られる" | "せる" | "させる"
                )
            })
}

pub(super) struct Predicate {
    /// サ変動詞なら名詞、形容詞＋するなら形容詞の位置。
    pub head: usize,
    pub end: usize,
    pub object: Option<usize>,
}

// nwiizo-coding-style: 最初の述語までの限定した形を扱う。一般的な係り受けは推定しない;
// 語彙を広げる際は別主語・目的語・連体修飾・複合語の反例も加える。
pub(super) fn first_predicate(
    tokens: &[&Morpheme],
    start: usize,
    accepts_object: impl Fn(&Morpheme) -> bool,
) -> Option<Predicate> {
    let mut object = None;
    for index in start..tokens.len().min(start + 20) {
        let token = tokens[index];
        let suru = tokens
            .get(index + 1)
            .is_some_and(|next| next.dictionary_form() == "する");
        if (token.pos(0) == "名詞" && token.pos(2).starts_with("サ変") && suru)
            || (token.pos(0) == "形容詞" && token.pos(5).starts_with("連用形") && suru)
        {
            return Some(Predicate {
                head: index,
                end: index + 1,
                object,
            });
        }
        if token.pos(0) == "動詞" {
            // 呼び出し元・移動先のような名詞の一部は述語ではない。
            if token.pos(5).starts_with("連用形")
                && tokens.get(index + 1).is_some_and(|suffix| {
                    matches!(suffix.pos(0), "名詞" | "接尾辞")
                        && matches!(suffix.normalized(), "元" | "先")
                })
            {
                continue;
            }
            return Some(Predicate {
                head: index,
                end: index,
                object,
            });
        }
        match token.pos(0) {
            "助詞" => match token.surface.as_str() {
                // 「じっとりと」「ふわりと」の「と」は副詞の一部で、並列や引用ではない。
                "と" if index > start && tokens[index - 1].pos(0) == "副詞" => {}
                "が" | "は" | "も" | "と" | "って" => return None,
                "を" => {
                    let head = index.checked_sub(1)?;
                    if object.is_some() || !accepts_object(tokens[head]) {
                        return None;
                    }
                    object = Some(head);
                }
                _ if token.pos(1) == "接続助詞" => return None,
                _ => {}
            },
            "助動詞" if token.surface == "に" => {}
            "形容詞" if token.pos(5).starts_with("連用形") => {}
            "名詞" | "形状詞" | "副詞" | "接頭辞" | "接尾辞" => {}
            "補助記号" if token.pos(1) == "読点" => {}
            _ => return None,
        }
    }
    None
}
