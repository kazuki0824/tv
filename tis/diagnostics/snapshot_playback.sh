#!/usr/bin/env bash
# 発動後のownerと別プロセスを採る。停止前の状態を復元する資料ではない。
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
# dumpsys/debuggerdには失敗文言を出して0終了する実装もある。
collect() {
    local file=$1 result=0 category=ok
    shift
    timeout 20s "$@" > "$out/$file.txt" 2>&1 || result=$?
    if (( result == 124 )); then category=timeout
    elif grep -Eqi "Permission Denial|permission denied|not permitted" "$out/$file.txt"; then category=permission-denied
    elif grep -Eqi "Can't find service|not found|No such process" "$out/$file.txt"; then category=not-found
    elif (( result != 0 )); then category=command-failed
    elif ! grep -q '[^[:space:]]' "$out/$file.txt"; then category=empty-output
    elif grep -Eqi '(^|[[:space:]])(error:|failed to|failure|timed out|DUMP TIMEOUT)' "$out/$file.txt"; then category=error-output
    fi
    printf '%s\t%s\t%s\n' "$file" "$result" "$category" > "$out/$file.status"
    [[ $category == ok ]]
}
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
            collect "native-$pid" adb shell debuggerd -b "$pid"
        ) &
        pids+=("$!")
    done
done
for job in "${pids[@]}"; do wait "$job" || status=1; done
# Codec2利用端末でmedia.codecが登録されているとは仮定しない。
collect services adb shell dumpsys -l || status=1
services=(tv_input media.resource_manager media.audio_flinger SurfaceFlinger android.hardware.tv.tuner.ITuner/default)
if grep -Eq '^[[:space:]]*media.codec[[:space:]]*$' "$out/services.txt"; then
    services+=(media.codec)
else
    printf 'media.codec\t-\tnot-found (optional; use media.resource_manager and codec process backtrace)\n' >> "$out/dumpsys-status.tsv"
fi
for service in "${services[@]}"; do
    file=${service//\//_}
    collect "$file" adb shell dumpsys "$service" || status=1
    cat "$out/$file.status" >> "$out/dumpsys-status.tsv"
done
echo "保存先: $out。*.status と dumpsys-status.tsv は対象・終了値・出力分類です。"
echo 'media.codecは登録時のみ採取します。Codec内部の待機はcodec processのbacktraceと継続traceで調べます。'
echo '必須対象の不在、権限拒否、timeout、空出力、エラー出力、非0終了は採取失敗です。'
exit "$status"
