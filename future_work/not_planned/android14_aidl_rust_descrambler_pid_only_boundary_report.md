# Android 14 AIDL/Rust nullable filter 境界の構造課題報告

## 1. 報告対象

本報告は、Android 14 系 Tuner HAL AIDL を Rust backend で実装する際に、AOSP framework / JNI / HIDL 側には存在する nullable filter 意味論を、official Rust generated trait だけでは受け取れない構造課題を同一ファイルで管理する。

同根として扱う対象は次の2件である。

| 対象API | AOSP側の意味論 | Android 14 AIDL/Rust backend 上の制約 |
|---|---|---|
| `IDescrambler.addPid()` / `removePid()` | null source filter / PID-only 経路 | Rust generated trait が non-null `Strong<dyn IFilter>` として現れ、Rust HAL public method だけでは null を受け取れない |
| `IFilter.setDataSource()` | `source == null` で入力元を demux に戻す | Rust generated trait が non-null `Strong<dyn IFilter>` として現れ、Rust HAL public method だけでは null を受け取れない |

future work へ同根課題を追加する場合は、本ファイル内に追記し、別ファイルへ分散させない。

## 2. 対象 Android バージョン

本プロジェクトの開発規則上、対象 Android は `get_android_qcow2.sh` で取得・ビルドする LineageOS 21 / Android 14 系に固定される。

したがって、本件は Android 14 系 Tuner HAL AIDL と、その Rust backend 生成物を前提に評価する。

## 3. AOSP 側の契約

### 3.1 `IDescrambler.addPid()` / `removePid()`

AOSP framework API 側では、`Descrambler.addPid()` / `removePid()` の `Filter` 引数は nullable として扱われる。JNI / native 側にも、filter が null の場合に null filter client として `addPid()` / `removePid()` へ渡す経路が存在する。

この意味論では、PID-only / null source filter 経路は実在する。

### 3.2 `IFilter.setDataSource()`

AOSP framework API / HIDL 契約では、`Filter.setDataSource(null)` / `IFilter.setDataSource(NULL)` は入力元を demux に戻す意味を持つ。

この意味論では、upstream filter source を設定した後に null source を渡して demux source へ復帰する経路は実在する。

## 4. Android 14 Tuner HAL AIDL / Rust backend 側の制約

Android 14 系の official Tuner HAL AIDL では、上記2 API の filter 引数が Rust backend で `Option<Strong<dyn IFilter>>` として現れない。

AIDL の `@nullable` 仕様では、Rust backend において `@nullable T` が `Option<T>` に写像される。interface 型でも、Rust backend で null を型上表現するには official AIDL 側で nullable として定義されている必要がある。

そのため、Android 14 系 official AIDL から生成された Rust trait が non-null `Strong<dyn IFilter>` 相当になる場合、Rust HAL 実装の public binder method だけでは null filter を受け取れない。

## 5. 開発規則との関係

本プロジェクトでは、Tuner HAL 改修は Rust 実装を原則とする。

次の対応は採用しない。

- AOSP stable / frozen AIDL に vendor 独自 `@nullable` を追加する。
- AOSP Tuner HAL AIDL の method signature を変更する。
- vendor 独自 AIDL method を追加して framework 経路を迂回する。
- C++ / NDK wrapper を追加して null Binder を受け、Rust 実装へ橋渡しする。
- Rust raw Binder transaction parser を手書きして official generated trait を迂回する。

C++ / native shim を認める例外は、FMQ / EventFlag / dma-buf など official native library への最小接続や、既存 C/C++ SDK への薄い FFI に限定される。Tuner HAL API 実装そのものの null Binder 受け口を追加することは、その例外には含めない。

## 6. 判定

本件は AOSP 標準との構造的な未達である。

ただし、現行の r51 Rust-only Tuner HAL 実装修正で完了可能なバグではない。

理由は、次の条件を同時に満たす実装経路が Android 14 Rust backend では成立しないためである。

```text
1. AOSP stable / frozen AIDL を変更しない。
2. C++ / NDK wrapper を追加しない。
3. Rust raw Binder transaction parser を追加しない。
4. null filter を public Rust Binder method で受ける。
```

したがって r51 では、nullable filter 経路を実装済み扱いにせず、Android 14 AIDL / Rust backend 境界の構造課題として本ファイルで追跡する。

## 7. r51 で実施すること

r51 では、Android 14 Rust generated trait で受け取れる non-null filter 経路だけを修正対象にする。

### 7.1 `IDescrambler.addPid()` / `removePid()`

- descrambler closed: `INVALID_STATE`
- demux 未設定: `INVALID_STATE`
- key token 未設定: `INVALID_STATE`
- demux generation 消失 / 再検査時 state 不整合: `INVALID_STATE`
- source filter closed / runtime-failed: `INVALID_STATE`
- invalid PID: `INVALID_ARGUMENT`
- foreign filter / 別 demux filter / dangling filter: `INVALID_ARGUMENT`
- unsupported `DemuxPid` variant: `UNAVAILABLE`

### 7.2 `IFilter.setDataSource()`

- non-null upstream source filter linkage を実装・確認する。
- demux default source を使う通常 filter path を維持する。
- `configure()` は既存 上流接続 を必ず clear する。
- closed / runtime-failed source または destination は `INVALID_STATE` とする。
- foreign / dangling / unsupported linkage は `INVALID_ARGUMENT` とする。
- `setDataSource(null)` を r51 実装済みとして記述しない。

## 8. r51 で実施しないこと

r51 では、次を実施しない。

- `IDescrambler.addPid()` / `removePid()` の `optionalSourceFilter == null` を PID-only として受ける実装。
- `IFilter.setDataSource(null)` を demux source 復帰として受ける実装。
- AOSP AIDL への `@nullable` 追加。
- vendor 独自 AIDL 追加。
- C++ / NDK wrapper 追加。
- Rust raw Binder transaction parser 追加。
- generated trait を迂回した nullable Binder 受け口追加。

## 9. 将来の解決条件

本件を将来実装対象に戻すには、次のいずれかが必要である。

1. 対象 Android / official Tuner HAL AIDL が、Rust backend で null filter を `Option<Strong<dyn IFilter>>` として受けられる形に更新される。
2. 開発規則を明示的に改訂し、対象API境界に限って C++ / NDK wrapper を例外許可する。
3. 開発規則を明示的に改訂し、Rust raw Binder transaction parser による generated trait 迂回を例外許可する。

現時点では、いずれも採用しない。

## 10. 完了判定への影響

r51 の完了判定では、本件を「実装修正済み」と扱わない。

r51 で確認するのは、Rust generated trait で到達できる non-null filter 経路の実装、error mapping、state cleanup、regression 防止に限定する。
