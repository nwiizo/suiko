use assert_cmd::Command;
use serde_json::Value;

const SHADOW_ARGS: &[&str] = &[
    "lint",
    "--profile",
    "remedy-seo",
    "--input-format",
    "html",
    "--format",
    "json",
    "--redact-excerpts",
    "-",
];
const FILLER_ARGS: &[&str] = &[
    "lint",
    "--profile",
    "remedy-seo-filler",
    "--input-format",
    "html",
    "--format",
    "json",
    "--redact-excerpts",
    "-",
];

fn remedy_binary() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("suiko-remedy"))
}

#[test]
fn version_json_uses_the_shared_remedy_identity() {
    let output = remedy_binary()
        .arg("--version-json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "name": "suiko-remedy",
            "version": "0.3.3-remedy.2",
            "commit": env!("SUIKO_REMEDY_COMMIT"),
        })
    );
}

#[test]
fn ordinary_suiko_arguments_are_rejected() {
    remedy_binary()
        .args(["lint", "README.md"])
        .assert()
        .code(1)
        .stdout("");
    remedy_binary().arg("--version").assert().code(1).stdout("");
}

#[test]
fn exact_shadow_profile_keeps_the_shared_schema_and_exit_two() {
    let output = remedy_binary()
        .args(SHADOW_ARGS)
        .write_stdin("<p>進めます。確認します。整えます。終えます。</p>")
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["schema_version"], "1");
    assert_eq!(value["findings"][0]["rule_id"], "masu-streak");
}

#[test]
fn exact_filler_profile_is_empty_for_other_categories() {
    remedy_binary()
        .args(FILLER_ARGS)
        .write_stdin(
            "<p>進めます。確認します。整えます。終えます。</p>\
             <p>一方で、Aです。</p><p>一方で、Bです。</p><p>一方で、Cです。</p>",
        )
        .assert()
        .success()
        .stdout("{\"schema_version\":\"1\",\"findings\":[]}\n");
}

#[test]
fn exact_filler_profile_emits_only_filler_and_exit_two() {
    let output = remedy_binary()
        .args(FILLER_ARGS)
        .write_stdin(format!("<p>{}</p>", "必要があります。".repeat(8)))
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["findings"].as_array().unwrap().len(), 1);
    assert_eq!(value["findings"][0]["rule_id"], "filler");
    assert_eq!(value["findings"][0].as_object().unwrap().len(), 4);
}
