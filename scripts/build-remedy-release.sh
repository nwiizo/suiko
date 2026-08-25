#!/bin/sh
# clean checkoutのHEADをfork identityとして注入するrelease build helper。
set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

if [ -n "$(git -C "$repo_root" status --porcelain --untracked-files=normal)" ]; then
    echo "error: remedy release build requires a clean checkout" >&2
    exit 1
fi

commit="$(git -C "$repo_root" rev-parse --verify HEAD)"
case "$commit" in
    "" | *[!0-9a-f]*)
        echo "error: git HEAD is not a canonical 40-character lowercase SHA" >&2
        exit 1
        ;;
esac
if [ "${#commit}" -ne 40 ]; then
    echo "error: git HEAD is not a canonical 40-character lowercase SHA" >&2
    exit 1
fi

cd "$repo_root"
export SUIKO_REMEDY_COMMIT="$commit"
cargo build --release --locked --bins "$@"

if [ -n "$(git status --porcelain --untracked-files=normal)" ]; then
    echo "error: remedy release build changed the checkout" >&2
    exit 1
fi
