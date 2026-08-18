use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

// 決定的な検出を保つため、埋め込む辞書を1つのSHA-256へ固定する。
// 辞書を更新する場合は scripts/fetch-dictionary.sh と同時に変更する。
const DICT_NAME: &str = "SudachiDict 20260723 core (system_core.dic)";
const DICT_SHA256: &str = "53fa281d11eef3769712fe1c3c892117338f9892bee6daf4dad51daa5281bb6f";
const DICT_ZIP_SHA256: &str = "b6e835f63440f97474c2da45d80950f73746e632e40bbfc168b4041729135e1f";
const DICT_ZIP_URL: &str =
    "https://d2ej7fkh96fzlu.cloudfront.net/sudachidict/sudachi-dictionary-20260723-core.zip";
const DICT_ZIP_ENTRY: &str = "system_core.dic";

fn sha256_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut output, "{byte:02x}").expect("write to String");
    }
    output
}

fn emit(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    let absolute = path.canonicalize().expect("canonicalize dictionary path");
    println!(
        "cargo:rustc-env=SUIKO_SUDACHI_DICT_FILE={}",
        absolute.display()
    );
}

fn verify(bytes: &[u8], origin: &str) {
    let actual = sha256_hex(bytes);
    if actual != DICT_SHA256 {
        panic!(
            "辞書のSHA-256が一致しません: {origin}\n  expected={DICT_SHA256}\n  actual  ={actual}\n\
             同じ入力に同じfindingを返すため、埋め込み辞書は {DICT_NAME} に固定しています。"
        );
    }
}

/// 公式配布のzipをSHA-256固定で取得し、system_core.dicをOUT_DIRへ展開する。
/// これはビルド時の1回だけで、実行時のダウンロードは発生しない。
fn download_dictionary(out_dir: &Path) -> PathBuf {
    let response = ureq::get(DICT_ZIP_URL)
        .call()
        .unwrap_or_else(|error| panic!("{DICT_NAME} のzip取得に失敗しました: {error}"));
    let mut zip_bytes = Vec::with_capacity(80 * 1024 * 1024);
    response
        .into_reader()
        .read_to_end(&mut zip_bytes)
        .expect("read dictionary zip");
    let actual = sha256_hex(&zip_bytes);
    if actual != DICT_ZIP_SHA256 {
        panic!(
            "辞書zipのSHA-256が一致しません:\n  expected={DICT_ZIP_SHA256}\n  actual  ={actual}"
        );
    }
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).expect("open dictionary zip archive");
    let mut dictionary = Vec::with_capacity(220 * 1024 * 1024);
    let mut found = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("read zip entry");
        if entry.name().ends_with(DICT_ZIP_ENTRY) {
            entry
                .read_to_end(&mut dictionary)
                .expect("extract dictionary entry");
            found = true;
            break;
        }
    }
    if !found {
        panic!("辞書zipに {DICT_ZIP_ENTRY} が見つかりません");
    }
    verify(&dictionary, "downloaded zip entry");
    let path = out_dir.join("system.dic");
    fs::write(&path, dictionary).expect("write dictionary to OUT_DIR");
    path
}

fn main() {
    println!("cargo:rerun-if-env-changed=SUIKO_SUDACHI_DICT");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));

    // docs.rsはネットワークなしでドキュメントだけを生成するため、空の
    // プレースホルダーを埋め込む(実行はされない)。
    if env::var_os("DOCS_RS").is_some() {
        let stub = out_dir.join("system.dic");
        fs::write(&stub, []).expect("write docs.rs stub");
        emit(&stub);
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    let candidates = [
        env::var_os("SUIKO_SUDACHI_DICT").map(PathBuf::from),
        Some(manifest_dir.join("resources/system.dic")),
        Some(out_dir.join("system.dic")),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.is_file() {
            let bytes = fs::read(&candidate).unwrap_or_else(|error| {
                panic!("辞書を読み込めません: {} ({error})", candidate.display())
            });
            if !bytes.is_empty() {
                verify(&bytes, &candidate.display().to_string());
                emit(&candidate);
                return;
            }
        }
    }

    if env::var("CARGO_NET_OFFLINE").as_deref() == Ok("true") {
        panic!(
            "{DICT_NAME} が見つかりません。オフラインビルドでは、\
             `./scripts/fetch-dictionary.sh` で resources/system.dic へ配置するか、\
             環境変数 SUIKO_SUDACHI_DICT で辞書ファイルを指定してください。"
        );
    }
    let downloaded = download_dictionary(&out_dir);
    emit(&downloaded);
}
