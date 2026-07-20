#!/usr/bin/env bash
set -euo pipefail

if (($# != 4)); then
  echo "Usage: $0 VERSION TAG INPUT_DIR OUTPUT_DIR" >&2
  exit 2
fi

version=$1
tag=$2
input_dir=$3
output_dir=$4
base_url="https://github.com/tyrival/Hummingbird-Releases/releases/download/$tag"

[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || { echo "Invalid version: $version" >&2; exit 1; }
[[ $tag == "v$version" ]] || { echo "Tag must equal v$version" >&2; exit 1; }
[[ -d $input_dir ]] || { echo "Input directory not found: $input_dir" >&2; exit 1; }
mkdir -p "$output_dir"
[[ -z $(find "$output_dir" -mindepth 1 -print -quit) ]] || { echo "Output directory must be empty: $output_dir" >&2; exit 1; }

one_file() {
  local root=$1 pattern=$2 label=$3
  local matches=()
  while IFS= read -r -d '' file; do matches+=("$file"); done < <(find "$root" -type f -name "$pattern" -print0)
  if ((${#matches[@]} != 1)); then
    echo "Expected exactly one $label ($pattern) below $root; found ${#matches[@]}." >&2
    exit 1
  fi
  printf '%s' "${matches[0]}"
}

copy_one() {
  local root=$1 pattern=$2 label=$3 name=$4 source
  source=$(one_file "$root" "$pattern" "$label") || exit 1
  cp "$source" "$output_dir/$name"
}

mac_arm_root="$input_dir/raw-macos-aarch64"
mac_x64_root="$input_dir/raw-macos-x86_64"
win_root="$input_dir/raw-windows-x86_64"
linux_root="$input_dir/raw-linux-x86_64"

mac_arm_dmg="Hummingbird_${version}_macos_aarch64.dmg"
mac_x64_dmg="Hummingbird_${version}_macos_x86_64.dmg"
mac_arm_update="Hummingbird_${version}_macos_aarch64.app.tar.gz"
mac_x64_update="Hummingbird_${version}_macos_x86_64.app.tar.gz"
win_update="Hummingbird_${version}_windows_x86_64-setup.exe"
linux_deb="Hummingbird_${version}_linux_x86_64.deb"
linux_update="Hummingbird_${version}_linux_x86_64.AppImage"

copy_one "$mac_arm_root" '*.dmg' 'macOS arm64 DMG' "$mac_arm_dmg"
copy_one "$mac_x64_root" '*.dmg' 'macOS x64 DMG' "$mac_x64_dmg"
copy_one "$mac_arm_root" '*.app.tar.gz' 'macOS arm64 updater bundle' "$mac_arm_update"
copy_one "$mac_arm_root" '*.app.tar.gz.sig' 'macOS arm64 updater signature' "$mac_arm_update.sig"
copy_one "$mac_x64_root" '*.app.tar.gz' 'macOS x64 updater bundle' "$mac_x64_update"
copy_one "$mac_x64_root" '*.app.tar.gz.sig' 'macOS x64 updater signature' "$mac_x64_update.sig"
copy_one "$win_root" '*.exe' 'Windows NSIS installer' "$win_update"
copy_one "$win_root" '*.exe.sig' 'Windows updater signature' "$win_update.sig"
copy_one "$linux_root" '*.deb' 'Linux DEB' "$linux_deb"
copy_one "$linux_root" '*.AppImage' 'Linux AppImage' "$linux_update"
copy_one "$linux_root" '*.AppImage.sig' 'Linux updater signature' "$linux_update.sig"

signature() {
  local file=$1 value
  value=$(tr -d '\r\n' < "$output_dir/$file.sig")
  [[ -n $value ]] || { echo "Empty signature: $file.sig" >&2; exit 1; }
  printf '%s' "$value"
}

export RELEASE_ASSET_VERSION="$version" RELEASE_ASSET_TAG="$tag" RELEASE_ASSET_URL="$base_url"
export MAC_ARM_UPDATE="$mac_arm_update" MAC_X64_UPDATE="$mac_x64_update" WIN_UPDATE="$win_update" LINUX_UPDATE="$linux_update"
export MAC_ARM_SIG="$(signature "$mac_arm_update")" MAC_X64_SIG="$(signature "$mac_x64_update")"
export WIN_SIG="$(signature "$win_update")" LINUX_SIG="$(signature "$linux_update")"
node <<'NODE' > "$output_dir/latest.json"
const fs = require('node:fs');
const out = process.env.RELEASE_OUTPUT_FILE;
const url = process.env.RELEASE_ASSET_URL;
const platform = (name, signature) => ({ signature, url: `${url}/${name}` });
const manifest = {
  version: process.env.RELEASE_ASSET_VERSION,
  notes: `Hummingbird ${process.env.RELEASE_ASSET_VERSION}`,
  pub_date: new Date().toISOString(),
  platforms: {
    'darwin-aarch64': platform(process.env.MAC_ARM_UPDATE, process.env.MAC_ARM_SIG),
    'darwin-x86_64': platform(process.env.MAC_X64_UPDATE, process.env.MAC_X64_SIG),
    'windows-x86_64': platform(process.env.WIN_UPDATE, process.env.WIN_SIG),
    'linux-x86_64': platform(process.env.LINUX_UPDATE, process.env.LINUX_SIG),
  },
};
process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
NODE

(
  cd "$output_dir"
  LC_ALL=C find . -maxdepth 1 -type f ! -name SHA256SUMS -print0 \
    | sort -z \
    | xargs -0 shasum -a 256 > SHA256SUMS
)

expected_count=13
actual_count=$(find "$output_dir" -maxdepth 1 -type f | wc -l | tr -d ' ')
[[ $actual_count == "$expected_count" ]] || { echo "Expected $expected_count release assets, found $actual_count." >&2; exit 1; }
echo "Prepared and validated $actual_count release assets in $output_dir."
