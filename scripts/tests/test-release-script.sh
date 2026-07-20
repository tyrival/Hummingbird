#!/usr/bin/env bash
set -euo pipefail

source_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/hummingbird-release-test.XXXXXX")
cleanup() { rm -rf "$fixture"; }
trap cleanup EXIT

repo="$fixture/repo"
remote="$fixture/remote.git"
mkdir -p "$repo/src-tauri" "$repo/scripts/tests" "$repo/.github/workflows" "$fixture/bin"
cp "$source_root/package.json" "$source_root/package-lock.json" "$repo/"
cp "$source_root/src-tauri/Cargo.toml" "$source_root/src-tauri/Cargo.lock" "$source_root/src-tauri/tauri.conf.json" "$repo/src-tauri/"
cp "$source_root/scripts/release.sh" "$source_root/scripts/check-no-secrets.sh" "$source_root/scripts/check-workflows.sh" "$source_root/scripts/prepare-release-assets.sh" "$repo/scripts/"
cp "$source_root/.github/workflows/ci.yml" "$source_root/.github/workflows/release.yml" "$repo/.github/workflows/"
chmod +x "$repo/scripts/"*.sh

cat > "$fixture/bin/npm" <<'SH'
#!/usr/bin/env bash
set -e
if [[ ${1:-} == install && ${2:-} == --package-lock-only ]]; then
  node -e "const fs=require('fs');const p=require('./package-lock.json');const v=require('./package.json').version;p.version=v;p.packages[''].version=v;fs.writeFileSync('package-lock.json',JSON.stringify(p,null,2)+'\n')"
fi
SH
cat > "$fixture/bin/cargo" <<'SH'
#!/usr/bin/env bash
exit 0
SH
cat > "$fixture/bin/git" <<'SH'
#!/usr/bin/env bash
if [[ ${FAIL_AT:-} == commit && ${1:-} == commit ]]; then exit 71; fi
if [[ ${FAIL_AT:-} == tag && ${1:-} == tag && ${2:-} == -a ]]; then exit 72; fi
if [[ ${FAIL_AT:-} == push && ${1:-} == push ]]; then exit 73; fi
exec /usr/bin/git "$@"
SH
chmod +x "$fixture/bin/"*

git init --bare "$remote" >/dev/null
git -C "$repo" init -b main >/dev/null
git -C "$repo" config user.name ReleaseTest
git -C "$repo" config user.email release@example.invalid
git -C "$repo" add .
git -C "$repo" commit -m initial >/dev/null
git -C "$repo" remote add origin "$remote"
git -C "$repo" push -u origin main >/dev/null
original=$(git -C "$repo" rev-parse HEAD)

assert_unchanged() {
  [[ $(git -C "$repo" rev-parse HEAD) == "$original" ]]
  [[ -z $(git -C "$repo" status --porcelain) ]]
  ! git -C "$repo" rev-parse -q --verify refs/tags/v1.2.3 >/dev/null
}

(cd "$repo" && PATH="$fixture/bin:$PATH" ./scripts/release.sh 1.2.3 --dry-run)
assert_unchanged

# The first public release may use the version already committed on main. In
# that case the helper must tag the current commit without inventing an empty
# release commit.
(cd "$repo" && PATH="$fixture/bin:$PATH" ./scripts/release.sh 0.1.0 --yes)
same_version_head=$(git -C "$repo" rev-parse HEAD)
[[ $same_version_head == "$original" ]] || {
  echo 'same-version release unexpectedly created a commit' >&2
  exit 1
}
[[ -z $(git -C "$repo" status --porcelain) ]]
[[ $(git --git-dir="$remote" rev-parse refs/tags/v0.1.0^{commit}) == "$original" ]]
git -C "$repo" tag -d v0.1.0 >/dev/null
git -C "$repo" push origin :refs/tags/v0.1.0 >/dev/null

for failure in commit tag push; do
  set +e
  (cd "$repo" && PATH="$fixture/bin:$PATH" FAIL_AT=$failure ./scripts/release.sh 1.2.3 --yes)
  status=$?
  set -e
  [[ $status -ne 0 ]] || { echo "$failure fixture unexpectedly succeeded" >&2; exit 1; }
  assert_unchanged
done

# Rollback leaves the checkout reusable.
(cd "$repo" && PATH="$fixture/bin:$PATH" ./scripts/release.sh 1.2.3 --dry-run)
assert_unchanged

echo 'Release script dry-run and commit/tag/push rollback fixtures passed.'
