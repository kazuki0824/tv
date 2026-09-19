#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# host 試験に必要な無改変の依存ソースと AOSP header を取得する。
set -euo pipefail

cas_work=${1:?Usage: prepare.sh WORK_DIRECTORY [YAKISOBA_DIRECTORY]}
cas_yakisoba=${2:-"$cas_work/libyakisoba"}
cas_revision=740c19b57aae0c6c678d414530ed98869ba08262

mkdir -p "$cas_work/headers"
if [[ ! -d "$cas_yakisoba/.git" ]]; then
    git init --quiet "$cas_yakisoba"
    GIT_TERMINAL_PROMPT=0 timeout 120 git -C "$cas_yakisoba" fetch --quiet --depth=1 \
        https://github.com/kazuki0824/libyakisoba-cross.git "$cas_revision"
    git -C "$cas_yakisoba" checkout --quiet --detach FETCH_HEAD
fi
[[ $(git -C "$cas_yakisoba" rev-parse HEAD) == "$cas_revision" ]]
git -C "$cas_yakisoba" diff --quiet HEAD

header() {
    local cas_name=$1 cas_project_path=$2 cas_digest=$3
    local cas_target="$cas_work/headers/$cas_name"
    if [[ -f "$cas_target" ]] && printf '%s  %s\n' "$cas_digest" "$cas_target" |
        sha256sum --check --status; then
        return
    fi
    mkdir -p "$(dirname "$cas_target")"
    curl --fail --silent --show-error --location --connect-timeout 15 --max-time 60 \
        "https://android.googlesource.com/platform/$cas_project_path?format=TEXT" |
        base64 --decode > "$cas_target.download"
    printf '%s  %s\n' "$cas_digest" "$cas_target.download" | sha256sum --check --status
    mv "$cas_target.download" "$cas_target"
}

header media/cas/CasAPI.h \
    frameworks/native/+/android-15.0.0_r1/headers/media_plugin/media/cas/CasAPI.h \
    ef3c44a6bf72bc11894095a0985b19e5251b6a7a695034de728badd97b541a12
header media/stagefright/MediaErrors.h \
    frameworks/av/+/android-15.0.0_r1/media/libstagefright/include/media/stagefright/MediaErrors.h \
    7e270b51e59c6b7f14e0a9ffd69c35f733425f5ec739faef3694a18d05d86d98
header utils/Errors.h \
    system/core/+/android-15.0.0_r1/libutils/binder/include/utils/Errors.h \
    001f9c16788f4dbe3a6d70f2a8e9b55d6928f91e086de498d86ad327d869ff19

printf 'Dependencies verified: %s\n' "$cas_revision"
