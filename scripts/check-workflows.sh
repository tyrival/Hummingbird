#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

fail() { echo "Workflow check failed: $*" >&2; exit 1; }
contains() { grep -Fq -- "$2" "$1" || fail "$1 is missing: $2"; }

for workflow in .github/workflows/ci.yml .github/workflows/release.yml; do
  [[ -f $workflow ]] || fail "missing $workflow"
  contains "$workflow" 'permissions:'
  contains "$workflow" 'contents: read'
  contains "$workflow" 'actions/checkout@v5'
  contains "$workflow" 'actions/setup-node@v5'
  contains "$workflow" 'tauri-apps/tauri-action@v1'
done

if rg -n 'actions/(checkout|setup-node)@v4' .github/workflows; then
  fail 'Node.js 20 based checkout/setup-node actions are no longer allowed'
fi

contains .github/workflows/ci.yml 'pull_request:'
contains .github/workflows/ci.yml 'branches: [main]'
contains .github/workflows/ci.yml 'workflow_dispatch:'
for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
  contains .github/workflows/ci.yml "$target"
  contains .github/workflows/release.yml "$target"
done

contains .github/workflows/release.yml "tags: ['v*']"
contains .github/workflows/release.yml 'tyrival/Hummingbird-Releases'
contains .github/workflows/release.yml "if: needs.validate.outputs.publish == 'true'"
contains .github/workflows/release.yml "if: needs.validate.outputs.publish != 'true'"
contains .github/workflows/release.yml "^(0|[1-9][0-9]*)\\\\.(0|[1-9][0-9]*)\\\\.(0|[1-9][0-9]*)$"
contains scripts/release.sh 'git worktree add --detach'
contains scripts/release.sh 'git push --atomic origin main "$tag"'
contains scripts/release.sh 'git reset --mixed "$original_head"'
contains scripts/release.sh '^(0|[1-9][0-9]*)'
for secret in RELEASES_REPO_TOKEN TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD; do
  contains .github/workflows/release.yml "secrets.$secret"
done
for extension in '*.dmg' '*.exe' '*.deb' '*.AppImage' '.sig' 'latest.json' 'SHA256SUMS'; do
  if ! grep -Fq -- "$extension" .github/workflows/release.yml && ! grep -Fq -- "$extension" scripts/prepare-release-assets.sh; then
    fail "release pipeline is missing: $extension"
  fi
done

if rg -n 'APPLE_(CERTIFICATE|CERTIFICATE_PASSWORD|API_KEY|API_ISSUER|TEAM_ID)|notar' .github/workflows; then
  fail 'Apple Developer ID/notarization configuration must not be present'
fi
if rg -n '^permissions:[[:space:]]*$|contents:[[:space:]]+write' .github/workflows/release.yml | grep -q 'write'; then
  fail 'source repository workflow must not request contents: write'
fi

# The cross-repository PAT is allowed only in the final publish step. Signing
# secrets are sufficient for tauri-action; builds must never receive the PAT.
pat_lines=$(grep -n 'secrets.RELEASES_REPO_TOKEN' .github/workflows/release.yml | cut -d: -f1)
[[ $(printf '%s\n' "$pat_lines" | grep -c .) -eq 1 ]] || fail 'PAT must occur exactly once, in final publish'
publish_line=$(grep -n '^  publish:' .github/workflows/release.yml | cut -d: -f1)
while IFS= read -r line; do
  [[ $line -gt $publish_line ]] || fail 'PAT leaked before final publish job'
  context=$(sed -n "$((line - 3)),$((line + 3))p" .github/workflows/release.yml)
  [[ $context == *'GH_TOKEN:'* ]] || fail 'PAT must be assigned only to GH_TOKEN in final publish step'
done <<< "$pat_lines"

echo 'Workflow static checks passed.'
