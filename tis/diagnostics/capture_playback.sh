#!/usr/bin/env bash
# 採取条件・解析方法の正本はtis/INTEGRATION.md。
set -euo pipefail
if (( $# < 1 || $# > 2 )); then
    echo "使用法: $0 新規出力ディレクトリ [採取秒数=120]" >&2
    exit 2
fi
out=$1
seconds=${2:-120}
[[ $seconds =~ ^[1-9][0-9]{0,2}$ ]] && (( seconds <= 600 )) || exit 2
command -v timeout >/dev/null
[[ $(adb shell id -u | tr -d '\r') == 0 ]] || { echo '先にadb rootを実行してください。' >&2; exit 1; }
mkdir -- "$out"
out=$(cd -- "$out" && pwd)
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
adb shell perfetto --query --long > "$out/perfetto-sources.txt" 2>&1
for source in android.surfaceflinger.layers android.surfaceflinger.frametimeline; do
    grep -Fq "$source" "$out/perfetto-sources.txt" || { echo "必要なdata sourceがありません: $source" >&2; exit 1; }
done
old_tis=$(adb shell getprop log.tag.MaleicacidTIS | tr -d '\r')
old_sync=$(adb shell getprop log.tag.MediaSync | tr -d '\r')
# 復元コマンドへ任意文字列を埋め込まない。
[[ $old_tis =~ ^[A-Za-z0-9]*$ && $old_sync =~ ^[A-Za-z0-9]*$ ]] || exit 1
log_pid=
trace_pid=
cleanup() {
    [[ -z $trace_pid ]] || kill "$trace_pid" 2>/dev/null || true
    if [[ -n $log_pid ]]; then
        kill "$log_pid" 2>/dev/null || true
        wait "$log_pid" 2>/dev/null || true
    fi
    adb shell "setprop log.tag.MaleicacidTIS '$old_tis'" || true
    adb shell "setprop log.tag.MediaSync '$old_sync'" || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM
remote="/data/misc/perfetto-traces/tis-playback-$(date +%s)-$$.perfetto-trace"
printf '%s\n' "$remote" > "$out/device-trace-path.txt"
adb shell getprop ro.build.fingerprint > "$out/build-fingerprint.txt"
git -C "$script_dir/../.." rev-parse HEAD > "$out/host-tv-revision.txt"
adb shell dumpsys package com.maleicacid.tvinput > "$out/package.txt"
adb shell setprop log.tag.MaleicacidTIS DEBUG
adb shell setprop log.tag.MediaSync DEBUG
adb logcat -b all -v threadtime -v monotonic > "$out/logcat.txt" 2> "$out/logcat.stderr" &
log_pid=$!
cat "$script_dir/playback.pbtxt" > "$out/config.pbtxt"
printf '\nduration_ms: %s000\n' "$seconds" >> "$out/config.pbtxt"
adb shell perfetto --txt -c - -o "$remote" < "$out/config.pbtxt" > "$out/perfetto.txt" 2>&1 &
trace_pid=$!
echo "採取中: $out（${seconds}秒）。選局前にperfetto.txtの開始を確認してください。"
echo "症状発生時は別端末で snapshot_playback.sh '$out' black などを実行してください。"
status=0
wait "$trace_pid" || status=$?
trace_pid=
printf 'perfetto\t%s\n' "$status" > "$out/status.tsv"
adb pull "$remote" "$out/playback.perfetto-trace" || status=1
[[ -s "$out/playback.perfetto-trace" ]] || status=1
kill -0 "$log_pid" 2>/dev/null || { echo 'logcatが採取中に終了しました。' >&2; status=1; }
echo "採取終了。traceのdata loss、native診断の有無、外部撮影との対応は解析時に確認します。"
exit "$status"
