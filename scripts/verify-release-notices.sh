#!/bin/sh
# Verify that a staged binary release contains the exact reviewed notices.
set -eu

if [ "$#" -ne 1 ] || [ ! -d "$1" ]; then
    echo "usage: verify-release-notices.sh RELEASE_DIRECTORY" >&2
    exit 2
fi

release_dir=$1
repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

compare() {
    source_file=$1
    packaged_file=$2
    if [ ! -f "$packaged_file" ] || ! cmp -s "$source_file" "$packaged_file"; then
        echo "error: release notice missing or changed: $packaged_file" >&2
        exit 1
    fi
}

compare "$repo_root/crates/suiko-sudachi/LICENSE" \
    "$release_dir/licenses/suiko-sudachi/LICENSE-2.0.txt"
compare "$repo_root/third_party/sudachidict/LEGAL" \
    "$release_dir/licenses/sudachidict/LEGAL"
compare "$repo_root/third_party/sudachidict/LICENSE-2.0.txt" \
    "$release_dir/licenses/sudachidict/LICENSE-2.0.txt"
compare "$repo_root/third_party/sudachidict/PROVENANCE.md" \
    "$release_dir/licenses/sudachidict/PROVENANCE.md"
