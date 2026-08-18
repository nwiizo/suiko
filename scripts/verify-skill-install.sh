#!/bin/sh
# 空の一時環境でAgent Skillの一覧取得と登録を検証する。
# ネットワークとnpxが必要なため、CIではなく手動・リリース前に実行する。
set -eu

REPO_URL="${1:-https://github.com/nwiizo/suiko}"
WORKDIR="$(mktemp -d)"
trap 'rm -rf "${WORKDIR}"' EXIT
cd "${WORKDIR}"

echo "==> 一覧取得: ${REPO_URL}"
npx -y skills add "${REPO_URL}" --list

echo "==> 登録: --skill suiko"
npx -y skills add "${REPO_URL}" --skill suiko

echo "==> 内容物の検証"
status=0
for file in SKILL.md agents/openai.yaml references/manual-checklist.md \
    references/diagnose.md assets/style-profile-template.md; do
    found="$(find . -path "*skills/suiko/${file}" | head -1)"
    if [ -n "${found}" ]; then
        echo "OK ${file}"
    else
        echo "MISSING ${file}" >&2
        status=1
    fi
done
exit "${status}"
