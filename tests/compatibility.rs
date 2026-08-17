use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

fn lint_fixture(name: &str, experimental: bool) -> Value {
    let path = format!("tests/fixtures/{name}");
    let mut command = cargo_bin_cmd!("suiko");
    command.args(["lint", &path, "--json"]);
    if experimental {
        command.arg("--experimental");
    }
    let output = command.output().expect("run suiko lint");
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).expect("valid JSON output")
}

#[test]
fn calibrated_fixtures_keep_their_finding_counts() {
    let smelly = lint_fixture("ai-smelly.md", false);
    let smelly_experimental = lint_fixture("ai-smelly.md", true);
    let natural = lint_fixture("natural.md", false);
    let natural_experimental = lint_fixture("natural.md", true);

    assert_eq!(smelly["stats"]["total_findings"], 25);
    assert_eq!(smelly_experimental["stats"]["total_findings"], 33);
    assert_eq!(natural["stats"]["total_findings"], 0);
    assert_eq!(natural_experimental["stats"]["total_findings"], 0);
}

#[test]
fn calibrated_categories_match_the_expected_profile() {
    let report = lint_fixture("ai-smelly.md", false);
    assert_eq!(report["stats"]["by_category"]["forbidden_phrase"], 8);
    assert_eq!(report["stats"]["by_category"]["antithesis_repetition"], 5);
    assert_eq!(report["stats"]["by_category"]["translationese"], 5);
    assert_eq!(report["stats"]["by_category"]["low_specificity"], 2);
    assert_eq!(report["stats"]["by_category"]["low_burstiness"], 1);
    assert_eq!(report["stats"]["by_category"]["inanimate_subject_morph"], 1);
}
