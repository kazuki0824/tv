# Android 14 AIDL/Rust backend 境界の構造課題報告

## 1. 報告対象

本報告は、r50aq 系で検出された `IDescrambler.addPid()` / `removePid()` の PID-only / null source filter 境界について、r50aq5 の実装対象から外して別管理する理由と、今後の扱いを固定するものである。

対象は Tuner HAL の `IDescrambler.addPid()` / `removePid()` である。

## 2. 対象 Android バージョン

本プロジェクトの開発規則上、対象 Android は `get_android_qcow2.sh` で取得・ビルドする LineageOS 21 / Android 14 系に固定される。

したがって、本件は Android 14 系 Tuner HAL AIDL と、その Rust backend 生成物を前提に評価する。

## 3. AOSP 側の契約

AOSP framework API 側では、`Descrambler.addPid()` / `removePid()` の `Filter` 引数は nullable として扱われる。

また、同一 PID に既存 filter が設定されている場合は、古い filter が新しく指定された filter に置換される、という説明がある。

参考:

- AOSP `Descrambler.java`
  - https://android.googlesource.com/platform/frameworks/base/+/master/media/java/android/media/tv/tuner/Descrambler.java

JNI / native 側にも、filter が null の場合に null filter client として `addPid()` / `removePid()` へ渡す経路が存在する。

参考:

- AOSP `android_media_tv_Tuner.cpp`
  - https://android.googlesource.com/platform/frameworks/base/+/master/media/jni/android_media_tv_Tuner.cpp

したがって、AOSP framework / JNI / VTS 側の意味論としては、PID-only / null source filter 経路は実在する。

## 4. Android 14 Tuner HAL AIDL / Rust backend 側の制約

一方で、Android 14 系の Tuner HAL AIDL では、`IDescrambler.addPid()` / `removePid()` の `optionalSourceFilter` は `@nullable` 付き引数ではない。

AIDL の `@nullable` 仕様では、Rust backend において `@nullable T` が `Option<T>` に写像される。特に `IBinder` や AIDL interface 型は、C++ / NDK backend では強ポインタ型のため型上 null を表せるが、Rust backend では `@nullable` が付いている場合だけ `Option<T>` になる。

参考:

- AIDL annotations: `@nullable`
  - https://source.android.com/docs/core/architecture/aidl/aidl-annotations
- AIDL for HALs
  - https://source.android.com/docs/core/architecture/aidl/aidl-hals

そのため、Android 14 系の official AIDL から生成された Rust trait が non-null `Strong<dyn IFilter>` 相当になる場合、Rust HAL 実装の public binder method だけでは null source filter を受け取れない。

## 5. 開発規則との関係

本プロジェクトでは、Tuner HAL 改修は Rust 実装を原則とする。

次の対応は採用しない。

- AOSP stable / frozen AIDL に vendor 独自 `@nullable` を追加する。
- AOSP `IDescrambler.aidl` の method signature を変更する。
- vendor 独自 AIDL method を追加する。
- C++ / NDK の `IDescrambler` wrapper を追加する。
- Rust raw Binder transaction parser を手書きして official generated trait を迂回する。

C++ / native shim を認める例外は、FMQ / EventFlag など official native library への最小接続や、既存 C/C++ SDK への薄い FFI に限定される。`IDescrambler.addPid()` / `removePid()` は Tuner HAL API 実装そのものなので、その例外には含めない。

## 6. 判定

本件は実バグである。

ただし、r50aq5 の Rust-only 実装修正で完了可能なバグではない。

理由は、次の4条件を同時に満たす実装経路が Android 14 Rust backend では成立しないためである。

```text
1. AOSP stable AIDL を変更しない。
2. C++ / NDK wrapper を追加しない。
3. Rust raw Binder transaction parser を追加しない。
4. null source filter を public Rust Binder method で受ける。
```

したがって r50aq5 では、PID-only / null source filter 経路を実装対象から外し、Android 14 AIDL / Rust backend 境界の構造課題として別管理する。

## 7. r50aq5 で実施すること

r50aq5 では、`IDescrambler.addPid()` / `removePid()` について、Android 14 Rust generated trait で受け取れる non-null source filter 経路の error mapping を修正対象にする。

具体的には次を修正対象とする。

- descrambler closed: `INVALID_STATE`
- demux 未設定: `INVALID_STATE`
- key token 未設定: `INVALID_STATE`
- demux generation 消失 / 再検査時 state 不整合: `INVALID_STATE`
- source filter closed / runtime-failed: `INVALID_STATE`
- invalid PID: `INVALID_ARGUMENT`
- foreign filter / 別 demux filter / dangling filter: `INVALID_ARGUMENT`
- unsupported `DemuxPid` variant: `UNAVAILABLE`

## 8. r50aq5 で実施しないこと

r50aq5 では、次を実施しない。

- `optionalSourceFilter == null` を PID-only として受ける実装。
- AOSP AIDL への `@nullable` 追加。
- vendor 独自 AIDL 追加。
- C++ / NDK `IDescrambler` wrapper 追加。
- Rust raw Binder transaction parser 追加。
- PID-only と source filter 付き登録の置換 semantics 実装。

## 9. 将来の解決条件

本件を将来実装対象に戻すには、次のいずれかが必要である。

1. 対象 Android / official Tuner HAL AIDL が、Rust backend で null source filter を `Option<Strong<dyn IFilter>>` として受けられる形に更新される。
2. 開発規則を明示的に改訂し、`IDescrambler` 境界に限って C++ / NDK wrapper を例外許可する。
3. 開発規則を明示的に改訂し、Rust raw Binder transaction parser による generated trait 迂回を例外許可する。

現時点では、いずれも採用しない。

## 10. 完了判定への影響

r50aq5 の完了判定では、本件を「実装修正済み」と扱わない。

r50aq5 の修正対象は、次の4件に限定する。

1. Issue 2: `IDescrambler.addPid/removePid` の non-null source filter 経路における error mapping 修正。
2. Issue 3: frontend scan lifecycle における active state と terminal diagnostic の分離。
3. Issue 4: section drop / stale partial discard の diagnostics / overflow status 接続。
4. 項目8: `DvrHal` close idempotency / cleanup retryability 修正。

本件は、Android 14 AIDL / Rust backend 境界の構造課題として、r50aq5 とは別に追跡する。
