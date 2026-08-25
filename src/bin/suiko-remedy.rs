use std::ffi::OsString;
use std::io::Read as _;
use std::process::ExitCode;

use serde::Serialize;
use suiko::remedy;

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

#[derive(Clone, Copy)]
enum AnalysisProfile {
    Shadow,
    Filler,
}

#[derive(Serialize)]
struct VersionOutput<'a> {
    name: &'static str,
    version: &'static str,
    commit: &'a str,
}

fn utf8_args() -> Result<Vec<String>, &'static str> {
    std::env::args_os()
        .skip(1)
        .map(|arg: OsString| {
            arg.into_string()
                .map_err(|_| "arguments must be valid UTF-8")
        })
        .collect()
}

fn profile(args: &[String]) -> Option<AnalysisProfile> {
    if args
        .iter()
        .map(String::as_str)
        .eq(SHADOW_ARGS.iter().copied())
    {
        Some(AnalysisProfile::Shadow)
    } else if args
        .iter()
        .map(String::as_str)
        .eq(FILLER_ARGS.iter().copied())
    {
        Some(AnalysisProfile::Filler)
    } else {
        None
    }
}

fn run() -> Result<ExitCode, String> {
    let args = utf8_args().map_err(str::to_owned)?;
    let commit = remedy::remedy_commit().map_err(str::to_owned)?;
    if args == ["--version-json"] {
        println!(
            "{}",
            serde_json::to_string(&VersionOutput {
                name: "suiko-remedy",
                version: remedy::REMEDY_VERSION,
                commit,
            })
            .map_err(|_| "version output serialization failed")?
        );
        return Ok(ExitCode::SUCCESS);
    }

    let selected = profile(&args).ok_or_else(|| {
        "exactly --version-json or an exact Remedy lint profile is required".to_owned()
    })?;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((remedy::MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "failed to read standard input")?;
    if bytes.len() > remedy::MAX_INPUT_BYTES {
        return Err(format!(
            "Remedy input exceeds {} bytes",
            remedy::MAX_INPUT_BYTES
        ));
    }
    let html = std::str::from_utf8(&bytes).map_err(|_| "Remedy input must be UTF-8")?;
    remedy::validate_html_input(html).map_err(str::to_owned)?;
    let output = match selected {
        AnalysisProfile::Shadow => remedy::analyze_html(html),
        AnalysisProfile::Filler => remedy::analyze_filler_html(html),
    };
    println!(
        "{}",
        serde_json::to_string(&output).map_err(|_| "report serialization failed")?
    );
    Ok(if output.findings.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("エラー: {message}");
            ExitCode::FAILURE
        }
    }
}
