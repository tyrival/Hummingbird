#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: ./scripts/release.sh VERSION [--dry-run] [--yes]

VERSION must be a stable x.y.z version. --dry-run verifies that target version
in an isolated temporary worktree and never edits, commits, tags, or pushes the
current checkout. --yes skips the interactive full-tag confirmation.
EOF
}

if [[ ${1:-} == -h || ${1:-} == --help ]]; then usage; exit 0; fi
version=${1:-}
[[ -n $version ]] || { usage >&2; exit 2; }
shift
dry_run=false
assume_yes=false
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=true ;;
    --yes) assume_yes=true ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[[ $version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || {
  echo 'VERSION must be a stable semantic version such as 1.2.3.' >&2
  exit 2
}

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

resolve_cargo() {
  if [[ -n ${HUMMINGBIRD_CARGO:-} ]]; then
    [[ -x $HUMMINGBIRD_CARGO ]] || {
      echo "HUMMINGBIRD_CARGO is not executable: $HUMMINGBIRD_CARGO" >&2
      return 1
    }
    printf '%s\n' "$HUMMINGBIRD_CARGO"
    return 0
  fi

  local discovered
  discovered=$(command -v cargo 2>/dev/null || true)
  if [[ -n $discovered ]]; then
    printf '%s\n' "$discovered"
    return 0
  fi

  local cargo_home=${CARGO_HOME:-}
  if [[ -z $cargo_home && -n ${HOME:-} ]]; then cargo_home=$HOME/.cargo; fi
  if [[ -n $cargo_home && -x $cargo_home/bin/cargo ]]; then
    printf '%s\n' "$cargo_home/bin/cargo"
    return 0
  fi

  echo 'Cargo was not found. Install Rust or set HUMMINGBIRD_CARGO to the cargo executable.' >&2
  return 1
}

cargo_bin=$(resolve_cargo)
[[ $(git branch --show-current) == main ]] || { echo 'Release must run on main.' >&2; exit 1; }
[[ -z $(git status --porcelain) ]] || { echo 'Working tree and index must be clean.' >&2; exit 1; }
git fetch origin main --tags
original_head=$(git rev-parse HEAD)
[[ $original_head == $(git rev-parse origin/main) ]] || { echo 'Local main must exactly match origin/main.' >&2; exit 1; }
tag="v$version"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && { echo "Tag $tag already exists." >&2; exit 1; }

update_versions() {
  local target_version=$1
  RELEASE_VERSION="$target_version" node <<'NODE'
const fs = require('node:fs');
const version = process.env.RELEASE_VERSION;
function atomic(file, content) {
  const temp = `${file}.release-tmp`;
  fs.writeFileSync(temp, content);
  fs.renameSync(temp, file);
}
for (const file of ['package.json', 'src-tauri/tauri.conf.json']) {
  const json = JSON.parse(fs.readFileSync(file, 'utf8'));
  json.version = version;
  atomic(file, `${JSON.stringify(json, null, 2)}\n`);
}
for (const file of ['src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']) {
  let text = fs.readFileSync(file, 'utf8');
  const pattern = file.endsWith('Cargo.toml')
    ? /^(\[package\]\n(?:.|\n)*?^version = ")[^"]+("$)/m
    : /(\[\[package\]\]\nname = "hummingbird"\nversion = ")[^"]+("\n)/;
  if (!pattern.test(text)) throw new Error(`hummingbird version not found in ${file}`);
  text = text.replace(pattern, `$1${version}$2`);
  atomic(file, text);
}
NODE
  npm install --package-lock-only --ignore-scripts
}

assert_versions() {
  local expected=$1 actual
  local versions=(
    "$(node -p "require('./package.json').version")"
    "$(node -p "require('./package-lock.json').version")"
    "$(node -p "require('./package-lock.json').packages[''].version")"
    "$(node -p "require('./src-tauri/tauri.conf.json').version")"
    "$(sed -n 's/^version = "\([^"]*\)"/\1/p' src-tauri/Cargo.toml | head -1)"
    "$(sed -n '/^name = "hummingbird"$/,+1 s/^version = "\([^"]*\)"/\1/p' src-tauri/Cargo.lock | head -1)"
  )
  for actual in "${versions[@]}"; do
    [[ $actual == "$expected" ]] || { echo "Version synchronization failed: expected $expected, found $actual" >&2; return 1; }
  done
}

sync_versions() {
  local target_version=$1
  if assert_versions "$target_version" >/dev/null 2>&1; then
    return 0
  fi
  update_versions "$target_version"
  assert_versions "$target_version"
}

run_checks() {
  ./scripts/check-no-secrets.sh
  ./scripts/check-workflows.sh
  npm ci
  npm run test:run
  npm run typecheck
  npm run lint
  npm run build
  "$cargo_bin" fmt --manifest-path src-tauri/Cargo.toml --all -- --check
  "$cargo_bin" clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
  "$cargo_bin" test --manifest-path src-tauri/Cargo.toml
}

if $dry_run; then
  dry_dir=$(mktemp -d "${TMPDIR:-/tmp}/hummingbird-release-dry.XXXXXX")
  cleanup_dry() { git -C "$repo_root" worktree remove --force "$dry_dir" >/dev/null 2>&1 || true; }
  trap cleanup_dry EXIT
  git worktree add --detach "$dry_dir" "$original_head" >/dev/null
  cd "$dry_dir"
  sync_versions "$version"
  run_checks
  git diff --check
  echo "Dry run for $tag passed in an isolated worktree; current checkout is unchanged."
  exit 0
fi

version_files=(package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json)
backup_dir=$(mktemp -d "${TMPDIR:-/tmp}/hummingbird-release-backup.XXXXXX")
for file in "${version_files[@]}"; do
  mkdir -p "$backup_dir/$(dirname "$file")"
  cp "$file" "$backup_dir/$file"
done
published=false
rollback() {
  local status=$?
  if ! $published; then
    if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then git tag -d "$tag" >/dev/null; fi
    if [[ $(git rev-parse HEAD) != "$original_head" ]]; then git reset --mixed "$original_head" >/dev/null; fi
    for file in "${version_files[@]}"; do cp "$backup_dir/$file" "$file"; done
    git reset --mixed "$original_head" >/dev/null
  fi
  rm -rf "$backup_dir"
  exit "$status"
}
trap rollback EXIT

sync_versions "$version"
run_checks
git diff --check

if ! $assume_yes; then
  printf 'Type %s to create and push the release: ' "$tag"
  read -r confirmation
  [[ $confirmation == "$tag" ]] || { echo 'Release cancelled.' >&2; exit 1; }
fi

if ! git diff --quiet -- "${version_files[@]}"; then
  git add -- "${version_files[@]}"
  git commit -m "release: $tag"
fi
git tag -a "$tag" -m "Hummingbird $version"
git push --atomic origin main "$tag"
published=true
echo "Published $tag. GitHub Actions will build and publish the installers."
