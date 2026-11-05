use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{lint, morphology::Morphology};

const CATEGORY: &str = "sensory_role_mismatch";

fn findings(body: &str, morphology: &Morphology, experimental: bool) -> Vec<lint::Finding> {
    lint::analyze(body, morphology, None, experimental)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.category == CATEGORY)
        .collect()
}

#[test]
fn sensory_roles_point_at_the_sense_argument_and_its_predicate() {
    let morphology = Morphology::new().unwrap();
    for (body, excerpt) in [
        ("雨音が窓を叩く。", "雨音が窓を叩く"),
        ("足音は廊下の床を叩いた。", "足音は廊下の床を叩い"),
        ("物音がガラス戸を叩いた。", "物音がガラス戸を叩い"),
        ("鐘の音が木の扉を叩きます。", "音が木の扉を叩き"),
        ("湯の熱さをじっと眺めた。", "熱さをじっと眺め"),
        ("器の冷たさを見つめる。", "冷たさを見つめる"),
        ("器の温かさを見詰めた。", "温かさを見詰め"),
        ("花の香りを静かに眺める。", "香りを静かに眺める"),
        ("においを眺める。", "においを眺める"),
        ("残った温もりを見つめた。", "温もりを見つめ"),
        ("静けさが大きく鳴った。", "静けさが大きく鳴っ"),
        ("静寂が部屋に響く。", "静寂が部屋に響く"),
        (
            "静けさが、鐘のように高く鳴る。",
            "静けさが、鐘のように高く鳴る",
        ),
    ] {
        let found = findings(body, &morphology, true);
        assert_eq!(
            found.len(),
            1,
            "{body}: {:?}",
            morphology.tokenize(body).unwrap()
        );
        let finding = &found[0];
        assert_eq!(finding.excerpt, excerpt, "{body}");
        assert_eq!(finding.severity, "info");
        assert!(finding.suggestion.is_none());
        let span = finding.span.unwrap();
        assert_eq!(&body[span.start_byte..span.end_byte], excerpt);
        assert_eq!(
            span.start_column,
            body[..span.start_byte].chars().count() + 1
        );
        assert_eq!(findings(body, &morphology, false).len(), 1);
    }
}

#[test]
fn sensory_roles_preserve_literal_senses_and_separate_clauses() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "雨粒が窓を叩く。",
        "下駄が廊下の床を叩く。",
        "雨音が部屋に響く。",
        "音が耳を叩く。",
        "歌声が心を叩く。",
        "静けさを破って鐘が鳴る。",
        "静寂の中で鈴が鳴った。",
        "静けさが戻ると、遠くで鐘が鳴った。",
        "静けさは続いたが、やがて鐘が鳴った。",
        "静けさが好きな彼は鐘を鳴らした。",
        "静けさがあっても彼には音が響く。",
        "静けさは鐘が鳴っても続いた。",
        "雨音が聞こえたので、私は扉を叩いた。",
        "雨音とともに、彼が扉を叩く。",
        "雨音のする窓を叩いた。",
        "冷たい器を見つめた。",
        "冷たさの残る器を眺める。",
        "冷たさを感じて、器を眺めた。",
        "冷たさを調べるために温度計を眺める。",
        "温かさを感じる。",
        "温度を測る。",
        "香りを嗅ぐ。",
        "香りのする花を眺める。",
        "熱さを示す目盛りを眺める。",
        "音が窓を叩かない。",
        "静けさが鳴りません。",
        "温もりを見つめていない。",
        "『静けさ』が会場に響く。",
        "「雨音が窓を叩く」と書かれている。",
        "「外は暗い。雨音が窓を叩く」と書かれている。",
        "「外は暗い。\n温もりを見つめる」と書かれている。",
        "雨音が窓を叩く音を聞いた。",
    ] {
        assert!(findings(body, &morphology, true).is_empty(), "{body}");
    }
}

#[test]
fn sensory_roles_respect_markdown_and_report_each_source_span() {
    let morphology = Morphology::new().unwrap();
    let masked = "# 雨音が窓を叩く\n\n> 静けさが鳴る。\n\n- 温もりを見つめる。\n\n`雨音が窓を叩く`\n\n```text\n静けさが鳴る。\n```\n";
    assert!(findings(masked, &morphology, true).is_empty());
    let body = "器を置いた。雨音が**窓を叩く**。\n\n温もりを見つめる。静けさが鳴る。\n";
    let found = findings(body, &morphology, true);
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), [1, 3, 3]);
    for finding in found {
        let span = finding.span.unwrap();
        let line = body.lines().nth(span.start_line - 1).unwrap();
        assert_eq!(&line[span.start_byte..span.end_byte], finding.excerpt);
    }
    assert_eq!(
        findings("「静けさが鳴る」。雨音が窓を叩く。", &morphology, true).len(),
        1
    );
}

#[test]
fn sensory_roles_support_genres_config_and_exit_status() {
    let morphology = Morphology::new().unwrap();
    let body = "雨音が窓を叩く。\n";
    for genre in [None, Some("essay"), Some("tech"), Some("business")] {
        assert!(
            lint::analyze(body, &morphology, genre, true)
                .unwrap()
                .findings
                .iter()
                .any(|f| f.category == CATEGORY)
        );
    }
    let dir = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "-", "--experimental", "--fail-on", "info", "--json"])
        .write_stdin(body)
        .assert()
        .code(2);
    std::fs::write(
        dir.path().join(".suiko.toml"),
        format!("version = 1\ndisabled_rules = [\"{CATEGORY}\"]\n"),
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
