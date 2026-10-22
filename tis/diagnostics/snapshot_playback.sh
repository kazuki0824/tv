#!/usr/bin/env bash
# 停止中のownerと別プロセスの待機相手を同じ症状区間で採る。
set -euo pipefail
if (( $# != 2 )); then
    echo "使用法: $0 採取ディレクトリ 症状名（英数字・ハイフン）" >&2
    exit 2
fi
[[ -d $1 && $2 =~ ^[A-Za-z0-9-]+$ ]] || exit 2
command -v timeout >/dev/null
[[ $(adb shell id -u | tr -d '\r') == 0 ]] || exit 1
out="$1/$2"
mkdir -- "$out"
adb shell log -t MaleicacidCapture "$2"
adb shell cat /proc/uptime > "$out/uptime.txt"
adb shell ps -A -T > "$out/threads.txt"
status=0
pids=()
# framework側のmediatunerプロセスが、TunerFilterのcloseとcallback Binderのロックを所有する。
for name in com.maleicacid.tvinput com.android.tv mediatuner system_server media.swcodec audioserver surfaceflinger android.hardware.tv.tuner-service.maleicacid2; do
    found=$(adb shell pidof "$name" | tr -d '\r') || found=
    if [[ -z $found ]]; then
        printf '%s\tnot-found\n' "$name" >> "$out/processes.tsv"
        status=1
        continue
    fi
    for pid in $found; do
        [[ $pid =~ ^[0-9]+$ ]] || exit 1
        printf '%s\t%s\n' "$name" "$pid" >> "$out/processes.tsv"
        (
            result=0
            timeout 20s adb shell debuggerd -b "$pid" > "$out/native-$pid.txt" 2>&1 || result=$?
            printf '%s\n' "$result" > "$out/native-$pid.status"
        ) &
        pids+=("$!")
    done
done
for job in "${pids[@]}"; do wait "$job"; done
for report in "$out"/native-*.status; do
    [[ -f $report && $(cat "$report") == 0 ]] || status=1
done
for service in tv_input media.codec media.audio_flinger SurfaceFlinger android.hardware.tv.tuner.ITuner/default; do
    file=${service//\//_}
    result=0
    timeout 20s adb shell dumpsys "$service" > "$out/$file.txt" 2>&1 || result=$?
    printf '%s\t%s\n' "$service" "$result" >> "$out/dumpsys-status.tsv"
    (( result == 0 )) || status=1
done
echo "保存先: $out。status非0・権限拒否・対象不在は採取成功と扱いません。"
exit "$status"
