#! /bin/bash

# set -x
set -euo pipefail

if [[ $# -ne 0 ]]; then
    echo "$0: expect no arguments" >&2
    exit 1
fi

cd "$(dirname "$0")"/..

DIR="$PWD"

cd "$(mktemp -d)"

git clone https://github.com/substrate-developer-hub/substrate-node-template .

git apply "$DIR"/cargo-test-fuzz/substrate_node_template.patch

git diff > "$DIR"/cargo-test-fuzz/substrate_node_template.patch
