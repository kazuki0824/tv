#!/usr/bin/env bash
# 採取条件・解析方法の正本はtis/INTEGRATION.md。
set -euo pipefail
if (( $# != 1 )); then
    echo "使用法: $0 新規出力ディレクトリ" >&2
    exit 2
fi
out=$1
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
[[ $old_tis =~ ^[A-Za-z0-9]*$ && $old_sync =~ ^[A-Za-z0-9]*$ ]] || exit 1
log_pid=
event_pid=
trace_started=0
key="tis-playback-$(date +%s)-$$"
remote="/data/misc/perfetto-traces/$key.perfetto-trace"
printf '%s\n' "$remote" > "$out/device-trace-path.txt"
printf '%s\n' "$key" > "$out/device-trace-session.txt"
cleanup() {
    if (( trace_started )); then
        adb shell perfetto --attach="$key" --stop >> "$out/perfetto.txt" 2>&1 || true
    fi
    [[ -z $event_pid ]] || kill "$event_pid" 2>/dev/null || true
    [[ -z $log_pid ]] || kill "$log_pid" 2>/dev/null || true
    adb shell "setprop log.tag.MaleicacidTIS '$old_tis'" || true
    adb shell "setprop log.tag.MediaSync '$old_sync'" || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM
adb shell getprop ro.build.fingerprint > "$out/build-fingerprint.txt"
git -C "$script_dir/../.." rev-parse HEAD > "$out/host-tv-revision.txt"
adb shell dumpsys package com.maleicacid.tvinput > "$out/package.txt"
adb shell setprop log.tag.MaleicacidTIS DEBUG
adb shell setprop log.tag.MediaSync DEBUG
adb logcat -b all -v threadtime -v monotonic > "$out/logcat.txt" 2> "$out/logcat.stderr" &
log_pid=$!
# 定期書出しを長周期にし、イベントまで中央バッファの古いデータを上書きする。
sed 's/file_write_period_ms: 1000/file_write_period_ms: 1000000000/' "$script_dir/playback.pbtxt" > "$out/config.pbtxt"
trace_started=1
adb shell perfetto --txt -c - --detach="$key" -o "$remote" < "$out/config.pbtxt" > "$out/perfetto.txt" 2>&1
adb shell perfetto --is_detached="$key" >/dev/null
# eventsはアクセシビリティ通知を出力する。端末側で利用可能か実機で確認する。
coproc EVENTS { exec adb shell uiautomator events 2> "$out/events.stderr"; }
event_pid=$EVENTS_PID
exec {events_fd}<&"${EVENTS[0]}"
echo "採取中: $out。Live TVの再生不能トーストを待ちます。"
echo '対象言語は英語・日本語です。eventsが端末で取得できることを確認してください。'
found=0
last_check=$SECONDS
seen_event=0
while kill -0 "$event_pid" 2>/dev/null && kill -0 "$log_pid" 2>/dev/null; do
    line=
    if IFS= read -r -t 5 -u "$events_fd" line; then
        if (( ! seen_event )); then
            echo 'アクセシビリティイベントを受信しました。'
            seen_event=1
        fi
        if [[ $line == *'TYPE_NOTIFICATION_STATE_CHANGED'* && $line == *'PackageName: com.android.tv;'* && $line == *'ClassName: android.widget.Toast;'* ]] &&
            [[ $line == *'Text: [The video is unexpectedly unavailable]'* || $line == *'Text: [動画を再生できなくなりました]'* ]]; then
            printf '%s\n' "$line" > "$out/trigger-event.txt"
            found=1
            break
        fi
    fi
    if (( SECONDS - last_check >= 5 )); then
        timeout 20s adb shell perfetto --is_detached="$key" >/dev/null || { echo 'Perfettoが途中終了しました。' >&2; exit 1; }
        last_check=$SECONDS
    fi
done
(( found )) || { echo 'イベント監視またはlogcatが終了しました。' >&2; exit 1; }
# stackの取得はtrace停止を待たず開始する。症状直後の5秒もリングに含める。
bash "$script_dir/snapshot_playback.sh" "$out" unavailable > "$out/snapshot.stdout" 2>&1 &
snapshot_pid=$!
sleep 5
status=0
if adb shell perfetto --attach="$key" --stop >> "$out/perfetto.txt" 2>&1; then
    trace_started=0
else
    status=1
fi
adb pull "$remote" "$out/playback.perfetto-trace" || status=1
[[ -s "$out/playback.perfetto-trace" ]] || status=1
wait "$snapshot_pid" || status=1
kill -0 "$log_pid" 2>/dev/null || { echo 'logcatが採取中に終了しました。' >&2; status=1; }
printf 'capture\t%s\n' "$status" > "$out/status.tsv"
echo '採取終了。イベントとtraceの前後区間、data lossを確認してください。'
exit "$status"
