#!/bin/sh
# SudachiDict を取得して resources/system.dic へ配置する。
# build.rs が同じSHA-256で辞書を検証するため、版を変える場合は両方を更新する。
set -eu

DICT_VERSION="20260723"
DICT_TYPE="core"
ZIP_SHA256="b6e835f63440f97474c2da45d80950f73746e632e40bbfc168b4041729135e1f"
DIC_SHA256="53fa281d11eef3769712fe1c3c892117338f9892bee6daf4dad51daa5281bb6f"

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

cd "$(dirname "$0")/.."
mkdir -p resources

if [ -f resources/system.dic ] && [ "$(sha256_of resources/system.dic)" = "${DIC_SHA256}" ]; then
    echo "resources/system.dic は既に配置済みです (SudachiDict ${DICT_VERSION} ${DICT_TYPE})"
    exit 0
fi

NAME="sudachi-dictionary-${DICT_VERSION}-${DICT_TYPE}"
WORKDIR="$(mktemp -d)"
trap 'rm -rf "${WORKDIR}"' EXIT

echo "Downloading ${NAME}.zip ..."
curl -fL "https://d2ej7fkh96fzlu.cloudfront.net/sudachidict/${NAME}.zip" \
    -o "${WORKDIR}/${NAME}.zip"

ACTUAL="$(sha256_of "${WORKDIR}/${NAME}.zip")"
if [ "${ACTUAL}" != "${ZIP_SHA256}" ]; then
    echo "zipのSHA-256が一致しません: expected=${ZIP_SHA256}, actual=${ACTUAL}" >&2
    exit 1
fi

unzip -o -j "${WORKDIR}/${NAME}.zip" -d "${WORKDIR}" >/dev/null
ACTUAL="$(sha256_of "${WORKDIR}/system_${DICT_TYPE}.dic")"
if [ "${ACTUAL}" != "${DIC_SHA256}" ]; then
    echo "辞書のSHA-256が一致しません: expected=${DIC_SHA256}, actual=${ACTUAL}" >&2
    exit 1
fi

mv "${WORKDIR}/system_${DICT_TYPE}.dic" resources/system.dic
echo "resources/system.dic (SudachiDict ${DICT_VERSION} ${DICT_TYPE}) を配置しました"
