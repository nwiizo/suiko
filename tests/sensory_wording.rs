use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{lint, morphology::Morphology};

fn sensory_findings(body: &str, genre: Option<&str>, experimental: bool) -> Vec<lint::Finding> {
    let morphology = Morphology::new().unwrap();
    lint::analyze(body, &morphology, genre, experimental)
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| f.category == "vague_sensory_term")
        .collect()
}

#[test]
fn sensory_terms_detect_predicates_and_basis_markers_in_every_genre() {
    for (body, excerpt) in [
        ("顧客の解像度を上げる。", "解像度を上げる"),
        ("課題の解像度が高い。", "解像度が高い"),
        ("議論の解像度が上がった。", "解像度が上がっ"),
        ("開発者の解像度が一段上がります。", "解像度が一段上がり"),
        ("課題の解像度が格段に高くなった。", "解像度が格段に高く"),
        ("説明を聞いて腹落ちしました。", "腹落ち"),
        ("この方針には腹落ち感がある。", "腹落ち感"),
        ("肌感として遅い。", "肌感として"),
        ("私の肌感覚では妥当だ。", "肌感覚で"),
        ("温度感を揃える。", "温度感を揃える"),
        ("チームの温度感が違う。", "温度感が違う"),
        ("先方と温度感を共有する。", "温度感を共有する"),
    ] {
        for genre in [Some("tech"), Some("business"), Some("essay"), None] {
            let findings = sensory_findings(body, genre, true);
            assert_eq!(findings.len(), 1, "{body}: {genre:?}: {findings:?}");
            let finding = &findings[0];
            assert_eq!(finding.excerpt, excerpt, "{body}");
            assert_eq!(finding.severity, "info");
            assert!(finding.suggestion.is_none());
            let span = finding.span.unwrap();
            assert_eq!(&body[span.start_byte..span.end_byte], excerpt);
            assert_eq!(
                span.start_column,
                body[..span.start_byte].chars().count() + 1
            );
        }
        assert!(sensory_findings(body, Some("business"), false).is_empty());
    }
}

#[test]
fn sensory_terms_preserve_literal_display_and_touch_uses() {
    for body in [
        "画面の解像度を上げる。",
        "写真の解像度が低い。",
        "解像度を1920x1080に上げる。",
        "4Kで解像度が高い。",
        "解像度は300dpiが高い。",
        "解像度という言葉を使う。",
        "解像度の設定を確認する。",
        "解像度が大事だ。",
        "画面の解像度が一段上がります。",
        "肌の感覚が鈍い。",
        "肌感覚を調べる研究だ。",
        "腹が減った。",
        "温度を揃える。",
        "温度感という語を説明する。",
        "> 顧客の解像度を上げる。",
        "`温度感を揃える`。",
        "# 顧客の解像度を上げる",
    ] {
        assert!(
            sensory_findings(body, Some("business"), true).is_empty(),
            "{body}"
        );
    }
}

#[test]
fn sensory_terms_support_disable_and_fail_on() {
    let body = "顧客の解像度を上げる。\n";
    let temp = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(temp.path())
        .args(["lint", "-", "--experimental", "--fail-on", "info", "--json"])
        .write_stdin(body)
        .assert()
        .code(2);
    std::fs::write(
        temp.path().join(".suiko.toml"),
        "version = 1\ndisabled_rules = [\"vague_sensory_term\"]\n",
    )
    .unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(temp.path())
        .args(["lint", "-", "--experimental", "--json"])
        .write_stdin(body)
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["category"] != "vague_sensory_term")
    );
}

#[test]
fn sensory_terms_report_each_term_once_with_related_lines() {
    let body = "腹落ち感がない。\n\n腹落ちする！腹落ちって大事。\n\n顧客の解像度を上げる。\n\n課題の解像度が低い。\n";
    let findings = sensory_findings(body, None, true);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert_eq!(findings[0].excerpt, "腹落ち感");
    assert_eq!(findings[0].related_lines, Some(vec![1, 3]));
    assert_eq!(findings[1].excerpt, "解像度を上げる");
    assert_eq!(findings[1].related_lines, Some(vec![5, 7]));
}

fn copy_findings(body: &str, experimental: bool) -> Vec<lint::Finding> {
    let morphology = Morphology::new().unwrap();
    lint::analyze(body, &morphology, Some("business"), experimental)
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| f.category == "copy_fragment")
        .collect()
}

#[test]
fn copy_fragments_detect_predicate_less_slogans() {
    for (body, excerpt) in [
        ("資料を、全員へ。", "資料を、全員へ"),
        ("作業を、もっと確かに。", "作業を、もっと確かに"),
        ("毎日の記録を、チームで。", "毎日の記録を、チームで"),
        ("次の一歩は、ここから。", "次の一歩は、ここから"),
        ("新しい資料を、全員へ。", "新しい資料を、全員へ"),
        ("設定の確認を、誰にでも。", "設定の確認を、誰にでも"),
    ] {
        let findings = copy_findings(body, true);
        assert_eq!(findings.len(), 1, "{body}: {findings:?}");
        assert_eq!(findings[0].excerpt, excerpt);
        assert_eq!(findings[0].severity, "info");
        assert!(findings[0].suggestion.is_none());
        let span = findings[0].span.unwrap();
        assert_eq!(&body[span.start_byte..span.end_byte], excerpt);
        assert!(copy_findings(body, false).is_empty());
    }
}

#[test]
fn copy_fragments_preserve_answers_predicates_and_headings() {
    for body in [
        "ええ、東京へ。",
        "東京へ。",
        "資料を、全員へ送ります。",
        "資料を、全員に配った。",
        "作業を、確かにする。",
        "資料を全員へ。",
        "会議の資料と議事録と関連する設計書を、全員へ。",
        "# 資料を、全員へ。",
        "> 資料を、全員へ。",
        "- 資料を、全員へ。",
        "資料を、全員へ？",
        "届いた資料を、全員へ。",
    ] {
        assert!(copy_findings(body, true).is_empty(), "{body}");
    }
}
