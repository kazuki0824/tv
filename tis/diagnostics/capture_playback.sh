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
log_pid=
event_pid=
snapshot_pid=
trace_started=0
properties_saved=0
status=1
old_tis=
old_sync=
key="tis-playback-$(date +%s)-$$"
remote="/data/misc/perfetto-traces/$key.perfetto-trace"
printf '%s\n' "$remote" > "$out/device-trace-path.txt"
printf '%s\n' "$key" > "$out/device-trace-session.txt"
cleanup() {
    local exit_status=$? result archive_dir archive
    trap - EXIT INT TERM
    set +e
    (( exit_status == 0 )) || status=$exit_status
    if (( trace_started )); then
        timeout 30s adb shell perfetto --attach="$key" --stop >> "$out/perfetto.txt" 2>&1
        result=$?
        printf 'trace-stop\t%s\n' "$result" >> "$out/status.tsv"
        (( result == 0 )) || status=1
        timeout 60s adb pull "$remote" "$out/playback.perfetto-trace" >> "$out/perfetto.txt" 2>&1
        result=$?
        printf 'trace-pull\t%s\n' "$result" >> "$out/status.tsv"
        (( result == 0 )) || status=1
    fi
    # 書込み中の子を残してアーカイブを成功扱いしない。
    for pid in "$event_pid" "$log_pid"; do
        if [[ -n $pid ]]; then
            kill "$pid" 2>/dev/null
            wait "$pid"
        fi
    done
    if [[ -n $snapshot_pid ]]; then
        wait "$snapshot_pid"
        result=$?
        printf 'snapshot\t%s\n' "$result" >> "$out/status.tsv"
        (( result == 0 )) || status=1
    fi
    if (( properties_saved )); then
        timeout 20s adb shell "setprop log.tag.MaleicacidTIS '$old_tis'"
        result=$?
        printf 'restore-tis-log\t%s\n' "$result" >> "$out/status.tsv"
        (( result == 0 )) || status=1
        timeout 20s adb shell "setprop log.tag.MediaSync '$old_sync'"
        result=$?
        printf 'restore-sync-log\t%s\n' "$result" >> "$out/status.tsv"
        (( result == 0 )) || status=1
    fi
    result=1
    if command -v trace_processor_shell >/dev/null && [[ -s $out/playback.perfetto-trace ]]; then
        sed "s/CAPTURE_KEY/$key/g" "$script_dir/coverage.sql" > "$out/coverage.sql"
        timeout 120s trace_processor_shell "$out/playback.perfetto-trace" -q "$out/coverage.sql" \
            > "$out/coverage.csv" 2> "$out/coverage.stderr"
        if (( $? == 0 )) && awk -F, 'NR > 1 && $1 == 1 {ok=1} END {exit !ok}' "$out/coverage.csv"; then result=0; fi
    else
        echo 'trace_processor_shellまたはtraceがなく、採取区間・欠落・nativeログを検証できません。' > "$out/coverage.stderr"
    fi
    printf 'evidence-coverage\t%s\n' "$result" >> "$out/status.tsv"
    (( result == 0 )) || status=1
    printf 'capture\t%s\n' "$status" >> "$out/status.tsv"
    echo "保存先: $out"
    cat "$out/status.tsv"
    if [[ -f $out/trigger-event.txt ]]; then cat "$out/trigger-event.txt"; else echo '対象イベント: 未捕捉'; fi
    # 排他的に作った場所だけに書き、失敗時は.partialのまま保持する。
    archive_dir=$(mktemp -d "${out}.archive.XXXXXX")
    if [[ -n $archive_dir ]]; then
        archive="$archive_dir/$(basename -- "$out").tar.gz"
        if tar -czf "$archive.partial" -C "$(dirname -- "$out")" -- "$(basename -- "$out")" &&
            mv -- "$archive.partial" "$archive"; then
            echo "archive: 0; 渡すファイル: $archive"
        else
            echo "archive: 1; アーカイブ作成失敗。取得済み資料: $out" >&2
            exit 1
        fi
    else
        echo "archive: 1; アーカイブ保存先を作成できません。取得済み資料: $out" >&2
        exit 1
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
adb shell perfetto --query --long > "$out/perfetto-sources.txt" 2>&1
for source in android.surfaceflinger.layers android.surfaceflinger.frametimeline android.log; do
    grep -Fq "$source" "$out/perfetto-sources.txt" || { echo "必要なdata sourceがありません: $source" >&2; exit 1; }
done
old_tis=$(adb shell getprop log.tag.MaleicacidTIS | tr -d '\r')
old_sync=$(adb shell getprop log.tag.MediaSync | tr -d '\r')
[[ $old_tis =~ ^[A-Za-z0-9]*$ && $old_sync =~ ^[A-Za-z0-9]*$ ]] || exit 1
properties_saved=1
adb shell getprop ro.build.fingerprint > "$out/build-fingerprint.txt"
git -C "$script_dir/../.." rev-parse HEAD > "$out/host-tv-revision.txt"
adb shell dumpsys package com.maleicacid.tvinput > "$out/package.txt"
# ホストHEADと端末成果物の由来を混同しない。対応manifestとsymbolsは同じbuildから別途保持する。
printf 'source-revision\tunverified; match device hashes to build artifacts/manifest/symbols\n' > "$out/device-provenance.tsv"
adb shell pm path com.maleicacid.tvinput | tr -d '\r' > "$out/apk-paths.txt"
[[ -s $out/apk-paths.txt ]] || exit 1
while IFS= read -r entry; do
    path=${entry#package:}
    [[ $entry == package:* && $path =~ ^/[A-Za-z0-9_./+=@:-]+$ ]] || exit 1
    adb shell sha256sum "$path" >> "$out/device-apk-sha256.txt"
done < "$out/apk-paths.txt"
grep -Eq '^[0-9a-f]{64}[[:space:]]' "$out/device-apk-sha256.txt" || exit 1
adb shell 'for path in /system/lib*/libstagefright.so /system/lib*/libmedia_jni.so /system/framework/framework.jar; do
    if [ -f "$path" ]; then sha256sum "$path" || exit 1; fi
done' > "$out/device-platform-sha256.txt"
grep -Eq '^[0-9a-f]{64}[[:space:]]' "$out/device-platform-sha256.txt" || exit 1
adb shell setprop log.tag.MaleicacidTIS DEBUG
adb shell setprop log.tag.MediaSync DEBUG
adb logcat -b all -v threadtime -v monotonic > "$out/logcat.txt" 2> "$out/logcat.stderr" &
log_pid=$!
# 症状開始が不明でも採取開始以降を残す。端末ファイルへ継続書出しする。
cp -- "$script_dir/playback.pbtxt" "$out/config.pbtxt"
trace_started=1
adb shell perfetto --txt -c - --detach="$key" -o "$remote" < "$out/config.pbtxt" > "$out/perfetto.txt" 2>&1
adb shell perfetto --is_detached="$key" >/dev/null
adb shell log -t MaleicacidCapture "capture-begin:$key"
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
adb shell log -t MaleicacidCapture "capture-trigger:$key"
# これはトースト後のsnapshotである。停止前の状態は継続traceとPLAYBACK_PRE_STOPで調べる。
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
snapshot_pid=
kill -0 "$log_pid" 2>/dev/null || { echo 'logcatが採取中に終了しました。' >&2; status=1; }

echo '採取終了。イベントとtraceの前後区間、data lossを確認してください。'
exit "$status"
