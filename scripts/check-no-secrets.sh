#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

failed=0

report_locations() {
  local label=$1
  local locations=$2
  if [[ -n "$locations" ]]; then
    # Never print the matching source line: CI logs must not echo the secret.
    printf 'ERROR: %s\n%s\n' "$label" "$locations" >&2
    failed=1
  fi
}

locations_for_pattern() {
  local pattern=$1
  shift
  LC_ALL=C awk -v pattern="$pattern" '$0 ~ pattern { print FILENAME ":" FNR }' "$@"
}

assignment_locations() {
  LC_ALL=C awk '
    /^[[:space:]]*(export[[:space:]]+)?(TAURI_SIGNING_PRIVATE_KEY|TAURI_SIGNING_PRIVATE_KEY_PASSWORD|RELEASES_REPO_TOKEN)[[:space:]]*=/ {
      value=$0
      sub(/^[^=]*=[[:space:]]*/, "", value)
      sub(/[[:space:]]*#[^\047\"]*$/, "", value)
      gsub(/^[\047\"]|[\047\"]$/, "", value)
      if (value != "" && value !~ /^\$\{/ && value !~ /^</ && value !~ /^REPLACE_/ && value !~ /^(your|example|placeholder)[_-]/) {
        print FILENAME ":" FNR
      }
    }
  ' "$@"
}

bearer_locations() {
  LC_ALL=C awk '
    {
      lowered=tolower($0)
      if (match(lowered, /bearer[[:space:]]+/)) {
        token=substr($0, RSTART + RLENGTH)
        sub(/[^A-Za-z0-9._~+\/=\-].*$/, "", token)
        if (length(token) >= 8) print FILENAME ":" FNR
      }
    }
  ' "$@"
}

scan_files() {
  local scan_failed_before=$failed
  local files=("$@")
  ((${#files[@]})) || return 0

  report_locations "发现 PEM 私钥内容" "$(locations_for_pattern \
    '-----BEGIN ([A-Z0-9 ]+ )?PRIVATE KEY-----' "${files[@]}")"
  report_locations "发现 Tauri/rsign/minisign 私钥内容" "$(locations_for_pattern \
    '([Uu]ntrusted comment: (rsign|minisign) (encrypted )?secret key|dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5|dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5)' "${files[@]}")"
  report_locations "发现疑似 GitHub Token" "$(locations_for_pattern \
    '(github_pat_[A-Za-z0-9_]+|gh[pousr]_[A-Za-z0-9]+)' "${files[@]}")"
  report_locations "发现 Bearer Token 字面值" "$(bearer_locations "${files[@]}")"
  report_locations "发现签名秘密或发布令牌的直接赋值" "$(assignment_locations "${files[@]}")"

  [[ $failed -eq $scan_failed_before ]]
}

run_self_test() {
  local test_dir
  test_dir=$(mktemp -d "${TMPDIR:-/tmp}/hummingbird-secret-check.XXXXXX")
  trap "rm -rf '$test_dir'" EXIT

  printf '%s\n' \
    'TAURI_SIGNING_PRIVATE_KEY=' \
    'TAURI_SIGNING_PRIVATE_KEY=${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}' \
    'RELEASES_REPO_TOKEN=<configured-in-github>' >"$test_dir/safe.env"
  failed=0
  if ! scan_files "$test_dir/safe.env" >/dev/null 2>&1; then
    printf 'ERROR: secret checker self-test rejected documented placeholders.\n' >&2
    return 1
  fi

  local case_name secret line output status
  for case_name in pat bearer rsign password; do
    case "$case_name" in
      pat)
        secret='github_pat_FAKESELFTEST0123456789abcdef'
        line=$secret
        ;;
      bearer)
        secret='fakeBearerSelfTest0123456789'
        line="Authorization: Bearer $secret"
        ;;
      rsign)
        secret='dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5'
        line=$secret
        ;;
      password)
        secret='fakeSigningPasswordSelfTest012345'
        line="TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$secret"
        ;;
    esac
    printf '%s\n' "$line" >"$test_dir/$case_name.txt"
    failed=0
    set +e
    output=$(scan_files "$test_dir/$case_name.txt" 2>&1)
    status=$?
    set -e
    if [[ $status -eq 0 ]]; then
      printf 'ERROR: secret checker self-test missed category %s.\n' "$case_name" >&2
      return 1
    fi
    if [[ "$output" == *"$secret"* ]]; then
      printf 'ERROR: secret checker self-test leaked a detected value.\n' >&2
      return 1
    fi
  done

  printf 'Secret checker self-test passed without leaking test values.\n'
}

if [[ ${1:-} == "--self-test" ]]; then
  run_self_test
  exit $?
fi
if (($#)); then
  printf 'Usage: %s [--self-test]\n' "$0" >&2
  exit 2
fi

cd "$repo_root"

# Include tracked/staged and non-ignored new source. Dependency/generated trees,
# implementation notes and this detector are excluded.
files=()
while IFS= read -r -d '' file; do files+=("$file"); done < <(
  git ls-files --cached --others --exclude-standard -z -- \
    ':!node_modules/**' ':!target/**' ':!src-tauri/target/**' ':!dist/**' \
    ':!.superpowers/**' ':!scripts/check-no-secrets.sh'
)

text_files=()
for file in "${files[@]}"; do
  [[ -f "$file" ]] || continue
  if LC_ALL=C grep -Iq . "$file"; then
    text_files+=("$file")
  fi
done

failed=0
scan_files "${text_files[@]}" || true

# Ignored private-key files are still forbidden. Report only their paths.
sensitive_files=()
while IFS= read -r -d '' file; do
  file=${file#./}
  sensitive_files+=("$file:0")
done < <(find . \
  \( -path './.git' -o -path './node_modules' -o -path './target' -o -path './src-tauri/target' -o -path './dist' \) -prune -o \
  -type f \( -name '*.key' -o -name '*.pem' -o -name '*.p12' -o -name '*.pfx' -o -name '*.minisign' -o -name '*.rsign' -o -name 'id_rsa' -o -name 'id_ed25519' \) \
  -print0)
if ((${#sensitive_files[@]})); then
  report_locations "发现禁止进入仓库的私钥文件" "$(printf '%s\n' "${sensitive_files[@]}")"
fi

# Migration fixtures are intentional; any other real legacy config is forbidden,
# including a config ignored by Git.
legacy_configs=()
while IFS= read -r -d '' file; do
  file=${file#./}
  case "$file" in
    tests/fixtures/config/*-config.txt) ;;
    *) legacy_configs+=("$file:0") ;;
  esac
done < <(find . \
  \( -path './.git' -o -path './node_modules' -o -path './target' -o -path './src-tauri/target' -o -path './dist' \) -prune -o \
  -type f -name 'config.txt' -print0)
if ((${#legacy_configs[@]})); then
  report_locations "发现旧版真实 config.txt" "$(printf '%s\n' "${legacy_configs[@]}")"
fi

if [[ $failed -ne 0 ]]; then
  exit 1
fi

printf 'No embedded secrets or legacy config files detected.\n'
