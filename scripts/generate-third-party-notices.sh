#!/usr/bin/env sh
# Regenerate the distributable license notice after dependency changes.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

cargo about generate --frozen --config about.toml third-party-notices.hbs > THIRD-PARTY-NOTICES.md
printf '\n\n' >> THIRD-PARTY-NOTICES.md
cat third-party-notices-extra.md >> THIRD-PARTY-NOTICES.md
