#!/bin/sh
# sudachi関連の更新を確認する。検出だけを行い、副作用はない。
# - sudachi.rs 上流の新しいタグ（現在の再配布は crates/suiko-sudachi/Cargo.toml の version）
# - 上流の crates.io 公式公開（公開されたら再配布crateから乗り換える）
# - SudachiDict の新しい版（現在のピンは scripts/fetch-dictionary.sh の DICT_VERSION）
# 更新があれば内容を標準出力へ書き、exit 1 を返す。最新なら exit 0。
set -eu

cd "$(dirname "$0")/.."

PINNED_SUDACHI="$(grep '^version' crates/suiko-sudachi/Cargo.toml | head -1 | cut -d'"' -f2)"
PINNED_DICT="$(grep '^DICT_VERSION=' scripts/fetch-dictionary.sh | cut -d'"' -f2)"

github_api() {
    if command -v gh >/dev/null 2>&1; then
        gh api "$1"
    else
        curl -fsSL -A "suiko-update-check (github.com/nwiizo/suiko)" "https://api.github.com/$1"
    fi
}

latest_sudachi="$(github_api repos/WorksApplications/sudachi.rs/tags \
    | python3 -c 'import json,sys; tags=[t["name"].lstrip("v") for t in json.load(sys.stdin)]; tags.sort(key=lambda v: [int(x) if x.isdigit() else 0 for x in v.replace("-",".").split(".")]); print(tags[-1] if tags else "")')"
latest_dict="$(github_api repos/WorksApplications/SudachiDict/releases/latest \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"].lstrip("v"))')"
crates_io_status="$(curl -s -o /dev/null -w '%{http_code}' \
    -A "suiko-update-check (github.com/nwiizo/suiko)" \
    "https://crates.io/api/v1/crates/sudachi")"

updates=0

if [ -n "${latest_sudachi}" ] && [ "${latest_sudachi}" != "${PINNED_SUDACHI}" ]; then
    updates=1
    echo "sudachi.rs: 上流に v${latest_sudachi} がある（再配布中: v${PINNED_SUDACHI}）。"
    echo "  対応: crates/suiko-sudachi を上流タグから作り直して再公開し、suikoの依存versionを上げる。"
fi

if [ "${crates_io_status}" = "200" ]; then
    updates=1
    echo "crates.io: 上流が公式に sudachi crate を公開した。 https://crates.io/crates/sudachi"
    echo "  対応: suikoの依存を公式crateへ切り替え、suiko-sudachiをdeprecatedにする（再配布時の約束）。"
fi

if [ -n "${latest_dict}" ] && [ "${latest_dict}" != "${PINNED_DICT}" ]; then
    updates=1
    echo "SudachiDict: ${latest_dict} が公開されている（ピン: ${PINNED_DICT}）。"
    echo "  対応: scripts/fetch-dictionary.sh と build.rs のSHA-256を同時に更新し、"
    echo "  eval/README.md の手順で report / labeled / sweep を全て再実行して差分を記録する。"
fi

if [ "${updates}" = "0" ]; then
    echo "更新なし: sudachi.rs v${PINNED_SUDACHI}, SudachiDict ${PINNED_DICT}, crates.io未公開(HTTP ${crates_io_status})。"
    exit 0
fi
exit 1
