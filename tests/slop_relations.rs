//! 意味の関係が崩れた表現（todesking「スロップ表現100連発」の型）を、自作の例で固定する。

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{lint, morphology::Morphology};

const CATEGORIES: &[&str] = &[
    "sensory_role_mismatch",
    "quantity_predicate_mismatch",
    "compressed_relation",
    "universal_exception_conflict",
];

fn findings(body: &str, morphology: &Morphology) -> Vec<lint::Finding> {
    lint::analyze(body, morphology, None, false)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| CATEGORIES.contains(&finding.category.as_str()))
        .collect()
}

#[test]
fn slop_relations_point_at_the_argument_and_its_predicate() {
    let morphology = Morphology::new().unwrap();
    for (body, category, excerpt) in [
        (
            "床から上がる温度が足首をじんわりと温めた。",
            "sensory_role_mismatch",
            "温度が足首をじんわりと温め",
        ),
        (
            "手のひらから、温度がゆっくり移っていく。",
            "sensory_role_mismatch",
            "温度がゆっくり移っ",
        ),
        (
            "温度が部屋に漂っていた。",
            "sensory_role_mismatch",
            "温度が部屋に漂っ",
        ),
        (
            "毎晩鳴っていたその不在が、部屋を広く見せた。",
            "sensory_role_mismatch",
            "鳴っていたその不在",
        ),
        (
            "棚の上で唸る不在が気になる。",
            "sensory_role_mismatch",
            "唸る不在",
        ),
        (
            "埃をかぶった光が床に落ちた。",
            "sensory_role_mismatch",
            "かぶった光",
        ),
        (
            "雪をまとった音が届く。",
            "sensory_role_mismatch",
            "まとった音",
        ),
        (
            "窓は、人々の疲れを一枚ずつ映している。",
            "quantity_predicate_mismatch",
            "疲れを一枚ずつ映し",
        ),
        (
            "彼の機嫌を二粒拾った。",
            "quantity_predicate_mismatch",
            "機嫌を二粒拾っ",
        ),
        (
            "判断に迷ったら安全側に相談する。",
            "compressed_relation",
            "安全側に相談",
        ),
        (
            "迷ったときは慎重側に聞く。",
            "compressed_relation",
            "慎重側に聞く",
        ),
        (
            "障害は責めず、仕組みを直す。",
            "compressed_relation",
            "障害は責め",
        ),
        (
            "インシデントを責めない文化を作る。",
            "compressed_relation",
            "インシデントを責め",
        ),
        (
            "棚の扉は全部閉まっている。ただ、左の扉だけが開いていた。",
            "universal_exception_conflict",
            "ただ、左の扉だけが開い",
        ),
        (
            "照明はすべて消えた。でも、廊下の明かりだけが点いている。",
            "universal_exception_conflict",
            "でも、廊下の明かりだけが点い",
        ),
        (
            "席は全部埋まった。しかし、窓際の席だけは埋まっていない。",
            "universal_exception_conflict",
            "しかし、窓際の席だけは埋まっ",
        ),
        // 状態を主語にとる「が」の形。
        (
            "退屈が一枚ずつ剥がれていく。",
            "quantity_predicate_mismatch",
            "退屈が一枚ずつ剥がれ",
        ),
        (
            "疲れが二粒こぼれた。",
            "quantity_predicate_mismatch",
            "疲れが二粒こぼれ",
        ),
        // 人の全称と、寝る・起きるの反対。
        (
            "社員はみんな寝ていた。ただ、私だけが起きていた。",
            "universal_exception_conflict",
            "ただ、私だけが起き",
        ),
        // 接続詞がなくても、「〜だけが」「〜だけは」で反対の状態を足す形。
        (
            "引き出しはすべて閉まっていた。一番下の引き出しだけが開いていた。",
            "universal_exception_conflict",
            "一番下の引き出しだけが開い",
        ),
        (
            "扉は全部閉まっている。右の扉だけが開いていた。",
            "universal_exception_conflict",
            "右の扉だけが開い",
        ),
    ] {
        let found = findings(body, &morphology);
        assert_eq!(
            found.len(),
            1,
            "{body}: {found:?} {:?}",
            morphology.tokenize(body).unwrap()
        );
        let finding = &found[0];
        assert_eq!(finding.category, category, "{body}");
        assert_eq!(finding.excerpt, excerpt, "{body}");
        assert_eq!(finding.severity, "info");
        assert!(finding.suggestion.is_none());
        let span = finding.span.unwrap();
        let line = body.lines().nth(span.start_line - 1).unwrap();
        assert_eq!(&line[span.start_byte..span.end_byte], excerpt, "{body}");
    }
}

#[test]
fn slop_relations_preserve_natural_wording_and_clause_boundaries() {
    let morphology = Morphology::new().unwrap();
    for body in [
        // 温度の値の変化、熱や気温、受け手の側の温かさ
        "部屋の温度が上がった。",
        "温度が急に下がる。",
        "手の温度が伝わる。",
        "温度を下げて保存する。",
        "熱が手のひらへ移っていく。",
        "低い気温が作物を冷やした。",
        "ぬくもりが手に移った。",
        "温度が上がると、氷が溶ける。",
        "温度が高くない日に部屋を暖める。",
        "ふわりと温度計が揺れた。",
        // 不在の継続・発生、別の名詞を修飾する節、否定
        "長く続いた不在が終わった。",
        "ずっと鳴っていた時計の不在が寂しい。",
        "突然訪れた不在に戸惑った。",
        "鳴らなかったその不在に気づいた。",
        // 物が身にまとう形、目的語としての光
        "埃をかぶった本を拾った。",
        "雪をかぶった山が光る。",
        "光をまとった少女が立つ。",
        // 状態を数えない、一つにまとめる、形のある物を数える
        "不安をひとつずつ解消する。",
        "緊張を一つ一つほぐす。",
        "疲れを取るために紙を一枚ずつ並べた。",
        "疲れを一枚ずつ映してはいない。",
        "疲れが二日残った。",
        "不安が一つ消えた。",
        "眠気が一粒の薬で消えた。",
        "緊張がほどけて、紙を一枚落とした。",
        // 方針の慣用句と、人・組織の側への相談
        "迷ったら安全側に倒す。",
        "会社側に相談する。",
        "相手側に問い合わせる。",
        // 人の失敗を責める、障害の原因を調べる
        "失敗を責めない。",
        "部下のミスを責めた。",
        "障害の原因を調べる。",
        // 例外が別の述語、否定の全称、例外の前置き、離れた段落
        "全員が来た。ただ、田中さんだけ遅れた。",
        "扉は全部閉まっているわけではない。ただ、右の扉だけが開いていた。",
        "全員が来た。田中さんだけ遅れた。",
        // 接続詞のない文では、「だけ」の後に別の主語が来る形（扉は閉まり、鍵が開く）を除く。
        "扉は全部閉まっている。右の扉だけ、鍵が開いていた。",
        "家族はみんな起きていた。祖母だけが早く起きていた。",
        "扉は全部閉まっている。ただ、右の扉も閉まっていた。",
        "扉は全部閉まっている。\n\n\nただ、右の扉だけが開いていた。",
        "「扉は全部閉まっている。ただ、右の扉だけが開いていた」と書いた。",
    ] {
        assert!(
            findings(body, &morphology).is_empty(),
            "{body}: {:?} {:?}",
            findings(body, &morphology),
            morphology.tokenize(body).unwrap()
        );
    }
}

#[test]
fn slop_relations_respect_markdown_masks_and_relate_the_universal_line() {
    let morphology = Morphology::new().unwrap();
    let masked = "# 温度が裾を暖める\n\n> 埃をかぶった光が落ちる。\n\n- 安全側に相談する。\n\n`疲れを一枚ずつ映す`\n";
    assert!(findings(masked, &morphology).is_empty());

    let body = "棚の扉は全部閉まっている。\nただ、左の扉だけが開いていた。\n";
    let found = findings(body, &morphology);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].line, 2);
    assert_eq!(found[0].related_lines, Some(vec![1]));
}

#[test]
fn slop_relations_support_genres_config_and_exit_status() {
    let morphology = Morphology::new().unwrap();
    let body = "棚の扉は全部閉まっている。ただ、左の扉だけが開いていた。\n";
    for genre in [None, Some("essay"), Some("tech"), Some("business")] {
        assert!(
            lint::analyze(body, &morphology, genre, false)
                .unwrap()
                .findings
                .iter()
                .any(|f| f.category == "universal_exception_conflict"),
            "{genre:?}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "-", "--fail-on", "info", "--json"])
        .write_stdin(body)
        .assert()
        .code(2);
    std::fs::write(
        dir.path().join(".suiko.toml"),
        "version = 1\ndisabled_rules = [\"universal_exception_conflict\"]\n",
    )
    .unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "-", "--fail-on", "info", "--json"])
        .write_stdin(body)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert!(report["findings"].as_array().unwrap().is_empty());
}
