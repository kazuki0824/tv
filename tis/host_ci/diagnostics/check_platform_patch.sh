#!/usr/bin/env bash
# 固定したplatform sourceへの適用を検査する。native buildの代替ではない。
set -euo pipefail
repo=$(pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
revision=c2d9d7b479e84a27c09468a41e39d7c6c58ae748
mkdir -p "$work/media/libstagefright/include/media/stagefright"
for path in media/libstagefright/MediaSync.cpp media/libstagefright/include/media/stagefright/MediaSync.h; do
    curl --fail --location --retry 2 "https://raw.githubusercontent.com/LineageOS/android_frameworks_av/$revision/$path" -o "$work/$path"
done
git -C "$work" init -q
git -C "$work" add .
git -C "$work" -c user.name=CI -c user.email=ci@example.invalid commit -qm baseline
patch_dir="$repo/tis/platform_patches/lineage-22.1"
git -C "$work" apply --check "$patch_dir/frameworks_av_mediasync_diagnostics.patch"
git -C "$work" apply "$patch_dir/frameworks_av_mediasync_first_output.patch"
git -C "$work" apply --check "$patch_dir/frameworks_av_mediasync_diagnostics.patch"
git -C "$work" apply "$patch_dir/frameworks_av_mediasync_diagnostics.patch"
git -C "$work" diff --check
