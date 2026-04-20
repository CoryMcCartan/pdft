#!/bin/bash
# Regenerate test fixture PDFs from Typst sources.
# Requires: typst (https://typst.app)
set -euo pipefail
cd "$(dirname "$0")"

for src in *.typ; do
    out="${src%.typ}.pdf"
    echo "$src → $out"
    typst compile "$src" "$out"
done
