# Android 14 AIDL/Rust nullable Binder 境界の阻害項目

## 1. 管理対象

本ファイルは、Android 14 系 Tuner HAL AIDL を Rust backend で実装する際に、AOSP framework / JNI / HIDL 側には存在する nullable Binder 意味論を、official Rust generated trait だけでは受け取れない構造課題を管理する。

本件は `not_planned` ではない。AOSP 契約の意味論としては対応対象である一方、Android 14 AIDL / Rust generated trait 境界で実装方式が未固定であるため、現行の AOSP 契約未達阻害項目として扱う。

同根として扱う対象は次の4件である。

| 対象API | AOSP側の意味論 | Android 14 AIDL/Rust backend 上の制約 |
|---|---|---|
| `IDescrambler.addPid()` / `removePid()` | null source filter / PID-only 経路 | Rust generated trait が non-null `Strong<dyn IFilter>` として現れ、Rust HAL public method だけでは null を受け取れない |
| `IFilter.setDataSource()` | `source == null` で入力元を demux に戻す | Rust generated trait が non-null `Strong<dyn IFilter>` として現れ、Rust HAL public method だけでは null を受け取れない |
| `IFrontend.setCallback()` | `callback == null` で callback 解除 | Rust generated trait が non-null `Strong<dyn IFrontendCallback>` として現れ、Rust HAL public method だけでは null を受け取れない |
| `ILnb.setCallback()` | `callback == null` で callback 解除 | Rust generated trait が non-null `Strong<dyn ILnbCallback>` として現れ、Rust HAL public method だけでは null を受け取れない |

nullable Binder 境界の同根課題を追加する場合は、現行リリース設計の正本である `tuner_hal/DESIGN_JA.md` へ吸収してから扱う。本ファイルは未解決条件の記録であり、現行設計判断、完了判定、実装済み範囲の正本ではない。

## 2. 対象 Android バージョン

本プロジェクトの開発規則上、対象 Android は `get_android_qcow2.sh` で取得・ビルドする LineageOS 21 / Android 14 系に固定される。

したがって、本件は Android 14 系 Tuner HAL AIDL と、その Rust backend 生成物を前提に評価する。

## 3. AOSP 側の契約

### 3.1 `IDescrambler.addPid()` / `removePid()`

AOSP framework API 側では、`Descrambler.addPid()` / `removePid()` の `Filter` 引数は nullable として扱われる。JNI / native 側にも、filter が null の場合に null filter client として `addPid()` / `removePid()` へ渡す経路が存在する。

この意味論では、PID-only / null source filter 経路は実在する。したがって、source filter を必須扱いすることは AOSP の意味論と一致しない。

### 3.2 `IFilter.setDataSource()`

AOSP framework API / HIDL 契約では、`Filter.setDataSource(null)` / `IFilter.setDataSource(NULL)` は入力元を demux に戻す意味を持つ。

この意味論では、upstream filter source を設定した後に null source を渡して demux source へ復帰する経路は実在する。したがって、demux source 復帰経路を恒久対象外として扱うことは AOSP の意味論と一致しない。

### 3.3 `IFrontend.setCallback()` / `ILnb.setCallback()`

AOSP AIDL では frontend callback および LNB callback は null 入力を許容し、既存 callback の解除として扱える意味論を持つ。

この意味論では、callback を必須扱いし、null による解除を実装済み範囲外として隠すことは AOSP の意味論と一致しない。ただし、Android 14 系 Rust generated trait が non-null `Strong<dyn ...Callback>` として現れる場合、Rust HAL public method だけでは null callback を受け取れない。

## 4. Android 14 Tuner HAL AIDL / Rust backend 側の制約

Android 14 系の official Tuner HAL AIDL では、上記2 API の filter 引数が Rust backend で `Option<Strong<dyn IFilter>>` として現れない。

AIDL の `@nullable` 仕様では、Rust backend において `@nullable T` が `Option<T>` に写像される。interface 型でも、Rust backend で null を型上表現するには official AIDL 側で nullable として定義されている必要がある。

そのため、Android 14 系 official AIDL から生成された Rust trait が non-null `Strong<dyn IFilter>` 相当になる場合、Rust HAL 実装の public binder method だけでは null filter を受け取れない。

現行実装の public method が `Strong<dyn IFilter>` を要求する限り、`setDataSource(NULL)` と `optionalSourceFilter == NULL` を Rust HAL public method の内部だけで実装済み扱いにしてはならない。

## 5. 開発規則との関係

本プロジェクトでは、Tuner HAL 改修は Rust 実装を原則とする。

次の対応は採用しない。

- AOSP stable / frozen AIDL に vendor 独自 `@nullable` を追加する。
- AOSP Tuner HAL AIDL の method 署名を変更する。
- vendor 独自 AIDL method を追加して framework 経路を迂回する。
- C++ / NDK ラッパーを追加して null Binder を受け、Rust 実装へ橋渡しする。
- Rust raw Binder transaction parser を手書きして official generated trait を迂回する。

C++ / ネイティブ薄層を認める例外は、FMQ / EventFlag / dma-buf など official native library への最小接続や、既存 C/C++ SDK への薄い FFI に限定される。Tuner HAL API 実装そのものの null Binder 受け口を追加することは、その例外には含めない。

上記禁止事項をすべて維持する場合、nullable filter を Rust HAL public method で受ける実装経路は固定できない。

## 6. 現行判定

本件は AOSP 標準との構造的な未達である。

ただし、現行の Rust-only Tuner HAL 実装修正だけで完了可能な通常バグではない。

理由は、次の条件を同時に満たす実装経路が Android 14 Rust backend では成立しないためである。

```text
1. AOSP stable / frozen AIDL を変更しない。
2. C++ / NDK ラッパーを追加しない。
3. Rust raw Binder transaction parser を追加しない。
4. null filter を public Rust Binder method で受ける。
```

したがって現行方針では、nullable filter 経路を実装済み扱いにしてはならない。本件は Android 14 AIDL / Rust backend 境界の nullable 未解決課題であり、現行の AOSP 契約未達阻害項目として追跡する。

## 7. 現行リリース側との関係

Rust generated trait で到達できる non-null filter 経路の現行処理、error mapping、state cleanup、regression 防止条件は `tuner_hal/DESIGN_JA.md` を正とする。

一方、本ファイルは次の境界について、Android 14 AIDL / Rust generated trait 上の未解決点と解決条件を記録する。

- `IDescrambler.addPid()` / `removePid()` の `optionalSourceFilter == null` を PID-only として受ける実装可否。
- `IFilter.setDataSource(null)` を demux source 復帰として受ける実装可否。
- `IFrontend.setCallback(null)` を callback 解除として受ける実装可否。
- `ILnb.setCallback(null)` を callback 解除として受ける実装可否。

上記境界の現行設計判断、capability / profile 方針、実装済み範囲、戻り値、状態遷移は `tuner_hal/DESIGN_JA.md` を正とする。本ファイルを現行リリース契約の正本として参照してはならない。

AOSP の意味論として上記境界は存在する。ただし Android 14 AIDL / Rust generated trait 境界で null filter を受け取る経路が未固定であるため、現行方針では次を実装済みとして扱わない。

- `IDescrambler.addPid()` / `removePid()` の `optionalSourceFilter == null` を PID-only として受ける実装。
- `IFilter.setDataSource(null)` を demux source 復帰として受ける実装。
- `IFrontend.setCallback(null)` を callback 解除として受ける実装。
- `ILnb.setCallback(null)` を callback 解除として受ける実装。
- AOSP AIDL への `@nullable` 追加。
- vendor 独自 AIDL 追加。
- C++ / NDK ラッパー追加。
- Rust raw Binder transaction parser 追加。
- generated trait を迂回した nullable Binder 受け口追加。

## 8. 解決条件

本件を実装済み扱いに戻すには、次のいずれかが必要である。

1. 対象 Android / official Tuner HAL AIDL が、Rust backend で null filter を `Option<Strong<dyn IFilter>>` として受けられる形に更新される。
2. 開発規則を明示的に改訂し、対象API境界に限って C++ / NDK ラッパーを例外許可する。
3. 開発規則を明示的に改訂し、Rust raw Binder transaction parser による generated trait 迂回を例外許可する。
4. AOSP framework / tuner service 側で null 経路を HAL へ到達させる、または HAL 到達前に AOSP 意味論を満たす公式経路を固定する。

現時点では、いずれも採用しない。したがって本件は 現行の AOSP 契約未達阻害項目として残る。

## 9. 未解決条件記録としての扱い

本ファイルは現行リリースの完了判定正本ではない。完了判定では、`tuner_hal/DESIGN_JA.md` に定義された現行実装済み範囲と、アーカイブ外の○×表を正とする。

本ファイルは、nullable Binder 境界を実装済み扱いに戻すための未解決条件を記録するためにだけ使う。本ファイルで確認対象として扱えるのは、Rust generated trait で到達できる non-null filter 経路の実装、error mapping、state cleanup、regression 防止に限定する。

AOSP 契約完全達成を主張するには、次をすべて満たす必要がある。

- `setDataSource(NULL)` が demux input 復帰として end-to-end で成立する。
- `addPid(pid, NULL)` が demux 入力全体の PID 指定として end-to-end で成立する。
- `removePid(pid, NULL)` が demux 入力全体の PID 登録解除として end-to-end で成立する。
- `IFrontend.setCallback(NULL)` が callback 解除として end-to-end で成立する。
- `ILnb.setCallback(NULL)` が callback 解除として end-to-end で成立する。
- 上記のために AOSP stable / frozen AIDL を vendor 独自改変していない。
- 上記のために開発規則で禁止された C++ / NDK ラッパー、vendor 独自 AIDL、Rust raw Binder transaction parser を無断追加していない。
- nullable 経路を実機または AOSP service 経路で確認できる。

上記を満たす実装方式が固定されるまで、現行リリース側の設計判断は `tuner_hal/DESIGN_JA.md` を正とし、本ファイルは未解決条件記録としてだけ扱う。
