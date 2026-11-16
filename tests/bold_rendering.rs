use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use suiko::{lint, morphology::Morphology};

const CATEGORY: &str = "bold_not_rendered";

fn findings(body: &str, morphology: &Morphology) -> Vec<lint::Finding> {
    lint::analyze(body, morphology, None, false)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.category == CATEGORY)
        .collect()
}

#[test]
fn bold_runs_that_commonmark_does_not_render_are_reported_with_their_span() {
    let morphology = Morphology::new().unwrap();
    for (body, excerpt) in [
        ("これは**「重要」**です。\n", "**「重要」**"),
        ("**「用語」**の説明を書く。\n", "**「用語」**"),
        ("**注意:**この操作は戻せない。\n", "**注意:**"),
        ("対象は**（旧）API**だ。\n", "**（旧）API**"),
        ("値は**`code`**です。\n", "**`code`**"),
        ("- **「重要」**を太字にする\n", "**「重要」**"),
        ("***注意:***この操作は戻せない。\n", "***注意:***"),
        ("これは__「重要」__です。\n", "__「重要」__"),
        ("これは__重要__です。\n", "__重要__"),
    ] {
        let found = findings(body, &morphology);
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        let finding = &found[0];
        assert_eq!(finding.severity, "warn");
        assert_eq!(finding.excerpt, excerpt);
        assert!(finding.suggestion.is_none());
        let span = finding.span.unwrap();
        assert_eq!(
            &body.lines().next().unwrap()[span.start_byte..span.end_byte],
            excerpt
        );
    }
}

#[test]
fn bold_runs_that_render_are_left_alone() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "これは **「重要」** です。\n",
        "これは**重要**です。\n",
        "**重要**です。\n",
        "**注意:** この操作は戻せない。\n",
        "**注意**: この操作は戻せない。\n",
        "「**重要**」と書く。\n",
        "```\nこれは**「重要」**です。\n```\n",
        "`**「重要」**`と書く。\n",
        "- **重要**を太字にする\n",
        "- **「重要」** を太字にする\n",
        "これは __重要__ です。\n",
        "***重要***です。\n",
    ] {
        let found = findings(body, &morphology);
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn bold_rendering_is_a_default_rule_that_config_can_disable() {
    assert!(lint::is_known_rule(CATEGORY));
    let dir = tempfile::tempdir().unwrap();
    let draft = dir.path().join("draft.md");
    std::fs::write(&draft, "これは**「重要」**です。\n").unwrap();
    let config = dir.path().join(".suiko.toml");
    std::fs::write(
        &config,
        "version = 1\ndisabled_rules = [\"bold_not_rendered\"]\n",
    )
    .unwrap();

    cargo_bin_cmd!("suiko")
        .args([
            "lint",
            draft.to_str().unwrap(),
            "--no-config",
            "--fail-on",
            "warn",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(CATEGORY));
    cargo_bin_cmd!("suiko")
        .args([
            "lint",
            draft.to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
            "--fail-on",
            "warn",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(CATEGORY).not());
}
