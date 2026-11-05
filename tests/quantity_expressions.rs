use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{lint, morphology::Morphology};

const CHARACTER_COUNT: &str = "quoted_character_count_mismatch";
const QUANTITY: &str = "quantity_predicate_mismatch";

fn findings(body: &str, morphology: &Morphology, experimental: bool) -> Vec<lint::Finding> {
    lint::analyze(body, morphology, None, experimental)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| matches!(finding.category.as_str(), CHARACTER_COUNT | QUANTITY))
        .collect()
}

#[test]
fn quoted_counts_compare_the_claim_with_the_visible_text() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "「青空」の四文字。",
        "『あいう』という二字です。",
        "「花の店　あお」の四文字",
        "「青空」の４文字だった。",
        "「春夏秋冬」という十二文字である。",
        "「ABC」の2字でした。",
        "「あいう」の十文字だ。",
        "「青 空」の四文字。",
    ] {
        let found = findings(body, &morphology, true);
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].category, CHARACTER_COUNT);
        assert_eq!(found[0].severity, "info");
        assert!(found[0].suggestion.is_none());
        assert!(found[0].detail.contains("字"));
        assert_eq!(findings(body, &morphology, false).len(), 1);
    }
    let found = findings("「青 空」の四文字。", &morphology, true);
    assert!(found[0].detail.contains("3字"));
    assert!(found[0].detail.contains("2字"));
    assert!(found[0].detail.contains("4字"));
}

#[test]
fn quantities_use_morphology_without_crossing_into_another_predicate() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "速度を数えた。",
        "ボールの速さを数えて記録した。",
        "川の長さを数える。",
        "気温を丁寧に数えます。",
        "荷物の重さを数えた。",
        "道の距離を数える。",
        "水の温度を数えた。",
        "肩の力を一つ抜いた。",
        "体の力をひとつ抜く。",
        "肩の力を１つ抜いていった。",
    ] {
        let found = findings(body, &morphology, true);
        assert_eq!(
            found.len(),
            1,
            "{body}: {found:?}; tokens: {:?}",
            morphology.tokenize(body).unwrap()
        );
        assert_eq!(found[0].category, QUANTITY);
        assert_eq!(found[0].severity, "info");
        assert!(found[0].suggestion.is_none());
        assert_eq!(findings(body, &morphology, false).len(), 1);
    }
}

#[test]
fn quantity_checks_leave_alternative_counts_and_natural_wording_alone() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "「青空」の二文字。",
        "「青 空」の三文字。",
        "「青 空」の二文字。",
        "「青空」の四文字以内。",
        "「青空」の四文字程度。",
        "「青空」の一文字を消す。",
        "「青空」の二文字目。",
        "「青空」の四文字ではない。",
        "「青空」の四文字ではありません。",
        "「青空」の四文字だったのではない。",
        "「青空」の四文字だろうか。",
        "「青空」の四文字か。",
        "「『青空』の四文字」と彼は書いた。",
        "「この説明は間違い。\n『青空』の四文字」と彼は書いた。",
        "「👩‍💻」の二文字。",
        "「か\u{3099}」の二文字。",
        "「葛\u{e0100}」の二文字。",
        "「ｶﾞ」の一文字。",
        "「**青空**」の二文字。",
        "「`海`」の三文字。",
        "歩数を数える。",
        "秒数を数える。",
        "距離を測る。",
        "速度を計算した。",
        "速さを測って人数を数える。",
        "温度を記録した回数を数える。",
        "速度の種類を数える。",
        "速度を変えた回数を数える。",
        "速度を彼は数える。",
        "速度は、回数を数えて求める。",
        "肩の力を抜く。",
        "力を一つにまとめる。",
        "力を一段抜く。",
        "力を抜いて一つ数えた。",
        "肩の力を一つ抜かない。",
        "速さを数えない。",
        "速さを数えません。",
        "速さを数えていない。",
        "速さを数えてはいけない。",
        "速さを数えてはならない。",
        "速さを数えはしない。",
        "速さを数えるべきではない。",
        "速さを数えるな。",
        "速さを数える必要はない。",
        "速さを数える方法は誤りだ。",
        "速さを数えるとは言っていない。",
        "「速さを数える」と書いてある。",
        "「外を眺めた。\n速さを数える」と書いてある。",
    ] {
        assert!(
            findings(body, &morphology, true).is_empty(),
            "{body}: {:?}",
            morphology.tokenize(body).unwrap()
        );
    }
}

#[test]
fn quantity_checks_preserve_markdown_masks_and_source_spans() {
    let morphology = Morphology::new().unwrap();
    let masked = "# 「青空」の四文字。\n\n> 速さを数える。\n\n- 肩の力を一つ抜く。\n\n`「青空」の四文字。`\n\n```text\n速さを数える。\n```\n";
    assert!(findings(masked, &morphology, true).is_empty());
    let body = "本を開く。「青空」の四文字。『あいう』という二字。\n\nまず、ボールの**速さ**を数えた。肩の力をひとつ抜く。\n";
    let found = findings(body, &morphology, true);
    assert_eq!(found.len(), 4, "{found:?}");
    assert_eq!(
        found.iter().map(|f| f.line).collect::<Vec<_>>(),
        [1, 1, 3, 3]
    );
    for finding in found {
        let span = finding.span.unwrap();
        let line = body.lines().nth(span.start_line - 1).unwrap();
        assert_eq!(&line[span.start_byte..span.end_byte], finding.excerpt);
        assert_eq!(
            span.start_column,
            line[..span.start_byte].chars().count() + 1
        );
    }
}

#[test]
fn quantity_checks_support_genres_config_and_cli_exit_status() {
    let morphology = Morphology::new().unwrap();
    let body = "「青空」の四文字。速さを数えた。\n";
    for genre in [None, Some("essay"), Some("tech"), Some("business")] {
        let report = lint::analyze(body, &morphology, genre, true).unwrap();
        for category in [CHARACTER_COUNT, QUANTITY] {
            assert!(report.findings.iter().any(|f| f.category == category));
        }
    }
    let dir = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "lint",
            "-",
            "--no-config",
            "--experimental",
            "--fail-on",
            "info",
        ])
        .write_stdin(body)
        .assert()
        .code(2);
    std::fs::write(
        dir.path().join(".suiko.toml"),
        format!("version = 1\ndisabled_rules = [\"{CHARACTER_COUNT}\", \"{QUANTITY}\"]\n"),
    )
    .unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "-", "--experimental", "--fail-on", "info", "--json"])
        .write_stdin(body)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert!(report["findings"].as_array().unwrap().is_empty());
}
