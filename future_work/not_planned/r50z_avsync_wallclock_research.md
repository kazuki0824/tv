# r50z A/V sync 再確認と wallclock 補間調査

## 0. 結論

`maleicacid_tv_r50z_refixed.tar.gz` は、r50z の A/V sync 受け入れ条件に対して小さな未達があった。

未達は、PCR 未観測時でも `getAvSyncHwId(Filter)` が valid ID を返し得る点である。AOSP CTS は `INVALID_AV_SYNC_ID` は許容するが、valid ID が返った場合は `getAvSyncTime(id)` が `INVALID_TIMESTAMP` ではないことを期待する。したがって、software demux が PCR 由来の source clock をまだ持っていない段階では、valid sync ID を先出ししない方が AOSP の期待に合う。

この未達は `maleicacid_tv_r50z_rerefixed.tar.gz` で修正した。

## 1. r50z 再確認結果

### 1.1 r50z refixed の状態

`r50z_refixed` では以下は実装済みだった。

- `IDemux.getAvSyncHwId(Filter)` は AV filter に deterministic sync ID を返す。
- `IDemux.getAvSyncTime(int)` は PCR base を 90kHz timestamp として返す。
- PCR 観測時刻から `elapsed_monotonic_time * 90000` を加算する最小 wallclock 補間が入っている。
- PCR 未観測時に PES PTS を current clock の代替にする fallback は削除済み。
- PCR 33-bit wrap は extended 90kHz 値へ伸長する。

### 1.2 未達

未達は以下である。

```text
AV filter configure 済み
PCR 未観測
getAvSyncHwId(filter) が valid ID を返す
getAvSyncTime(valid ID) は UNAVAILABLE / INVALID_TIMESTAMP になる
```

AOSP CTS の `testAvSyncId()` は、`getAvSyncHwId(f)` の戻りが `INVALID_AV_SYNC_ID` ではない場合、`getAvSyncTime(id)` が `INVALID_TIMESTAMP` ではないことを確認している。

### 1.3 再々修正版の修正内容

`maleicacid_tv_r50z_rerefixed.tar.gz` では以下に修正した。

- `av_sync_hw_id_for(filter_id)` は、対象が AV filter であることに加えて、PCR 由来の source clock が存在する場合だけ ID を返す。
- PCR 未観測時は `getAvSyncHwId()` 側で `UNAVAILABLE` になり、valid ID を先出ししない。
- `getAvSyncTime()` は valid ID 検証後、PCR 由来の current 90kHz timestamp だけを返す。
- PTS fallback は引き続き禁止。
- `DESIGN_JA.md` に、valid ID と valid timestamp の関係を明記した。
- unit test を、PCR 未観測時は sync ID なし、PCR 観測後に sync ID が出る期待に更新した。

## 2. AOSP 側の期待仕様

AOSP HIDL `IDemux` の仕様では、`getAvSyncTime()` は「A/V sync に使う current timestamp」を返し、hardware が current timestamp を increment / maintain する、と説明されている。timestamp は 90kHz base で、PTS と同じ format である。

AOSP Java `Tuner.getAvSyncTime()` も、timestamp は hardware によって maintain され、90kHz base で PTS と同じ format と説明している。

重要なのは、ここでいう PTS は **format の説明**であって、PES PTS を current A/V sync clock の代替として返してよいという意味ではない点である。

AOSP CTS は次の形で検査する。

```text
id = getAvSyncHwId(audioFilter)
if id != INVALID_AV_SYNC_ID:
    assert getAvSyncTime(id) != INVALID_TIMESTAMP
```

したがって、software demux が current clock をまだ持たない段階では、valid ID を返さない設計が安全である。

## 3. wallclock 補間とは何か

wallclock 補間は、最後に観測した PCR と、その PCR を観測したローカル monotonic clock 時刻を対応付け、その後の経過時間を 90kHz tick に換算して足す処理である。

```text
estimated_90khz = last_pcr_90khz + elapsed_monotonic_ns * 90000 / 1_000_000_000
```

r50z では、以下の最小実装でよい。

- PCR base を 90kHz timestamp として保持する。
- PCR 観測時の `Instant` を保持する。
- `getAvSyncTime()` 呼び出し時に monotonic 経過時間を 90kHz tick に換算して加算する。
- 線形回帰、PLL、drift compensation、skew correction は入れない。

## 4. 先人の例

### 4.1 GStreamer `mpegtslivesrc`

GStreamer の `mpegtslivesrc` は、MPEG-TS live source を wrap し、stream の PCR に基づく clock を提供する element である。GStreamer Rust plugins の README でも、`mpegtslivesrc` は `udpsrc` や `srtsrc` などの MPEG-TS source を wrap し、stream の PCR に基づく live clock を提供すると説明されている。

Centricular の開発記事では、`mpegtslivesrc` が in-stream PCR を使って sender clock time と local receive time を相関させ、linear regression で sender clock と local system clock の相対 rate を計算すると説明している。

これは r50z の最小補間より高度である。r50z は「最後の PCR + monotonic 経過時間」だけで、linear regression による drift 推定は行わない。

### 4.2 GStreamer `mpegtsdemux` / `MpegTSBase`

GStreamer の MPEG-TS demux 系には PCR を timing に使う設計がある。`ignore-pcr` という property も存在し、PCR を無視する設定があることから、PCR が timing の入力として扱われていることが分かる。

ただし、GStreamer の一般的な同期設計文書では、stream 内容から clock を生成することは可能だが推奨されない場面もある、とされている。これは、stream 由来 clock が jitter や破損 stream の影響を受けやすいためである。今回の Tuner HAL は AOSP が current A/V sync timestamp を要求するため、PCR 由来 clock を最小実装する合理性はあるが、長時間運用では drift correction の余地がある。

### 4.3 AOSP / ExoPlayer / Media3 の timestamp adjuster

ExoPlayer / Media3 系には MPEG-TS の 33-bit timestamp wrap を扱う timestamp adjuster がある。これは PTS/PCR 系 timestamp の wrap と offset 調整を扱うもので、r50z の `AvSyncTimestampExtender` と同種の問題を扱う。

ただし、これは AOSP Tuner HAL の `getAvSyncTime()` 参照実装ではない。再生 pipeline 内部で sample timestamp を扱うための部品であり、HAL の current A/V sync clock provider として as-is 利用するものではない。

### 4.4 FFmpeg 系

FFmpeg は MPEG-TS demux / mux で PCR/PTS/DTS を扱うが、今回調査範囲で「Android Tuner HAL の `getAvSyncTime()` にそのまま流用できる PCR→local wallclock clock discipline 部品」は確認できなかった。

## 5. AOSP 側に wallclock 補間の参照実装があるか

結論: **今回の用途にそのまま使える AOSP 参照実装は見つからない。**

確認できたものは以下である。

### 5.1 仕様/API 文書

- `Tuner.getAvSyncHwId(Filter)`
- `Tuner.getAvSyncTime(int)`
- HIDL `IDemux.getAvSyncTime()`
- JNI bridge
- CTS test

これらは期待仕様と API 挙動を示すが、PCR を wallclock 補間して current 90kHz timestamp を作る実装ではない。

### 5.2 HIDL default 実装の痕跡

古い HIDL default 実装では、PCR filter ID を A/V sync ID として扱うテスト用実装の痕跡がある。VTS 追加 commit では、media filter に対して PCR filter id を A/V sync id として返す default implementation の変更が見える。

ただし、この default 実装は実用的な wallclock 補間実装ではない。少なくとも今回の Android 14 AIDL HAL に as-is で使う clock discipline ではない。

### 5.3 AIDL VTS

Android 14 系の AIDL VTS functional tests には `DemuxTests::getAvSyncId()` / `getAvSyncTime()` helper は存在するが、調査した `VtsHalTvTunerTargetTest.cpp` には `getAvSync` を直接呼ぶ test は見当たらなかった。

一方で CTS には Java API レベルの `testAvSyncId()` がある。従って r50z では、VTS だけではなく CTS/API expectation も意識すべきである。

## 6. as-is で組み込み利用可能な OSS があるか

結論: **今回の HAL に as-is で組み込むべき OSS はない。**

### 6.1 GStreamer `mpegtslivesrc`

`mpegtslivesrc` は最も近い先例である。PCR と local receive time を対応付け、linear regression で sender clock を local GStreamer clock として近似する。

しかし、これは GStreamer plugin と GStreamer clock infrastructure に依存する。Android vendor Tuner HAL の Rust service に as-is で組み込むには、GStreamer runtime、plugin registry、GObject/GStreamer threading model、ライセンス・ビルド統合を持ち込む必要がある。r50z/r51 の Tuner HAL に直接組み込むには重すぎる。

採用可能なのは **アルゴリズム上の参考**である。

### 6.2 ExoPlayer / Media3 `TimestampAdjuster`

Apache-2.0 であり Android 親和性は高いが、これは Java/Kotlin 側の media extractor 向け timestamp 調整部品である。HAL 内部の Rust demux に as-is 組み込みするには向かない。

採用可能なのは **33-bit wrap handling と offset adjustment の考え方**である。

### 6.3 Rust MPEG-TS parser crates

`mpegts` や `mpeg2ts-reader` のような Rust crate は、MPEG-TS packet / adaptation field / PCR parse の参考にはなる。ただし、Android platform build / Soong / vendor HAL へ外部 crate を追加するコスト、保守性、既存 soft_demux との重複を考えると、r50z で as-is 採用する利点は小さい。

今回の実装は PCR 6 byte の抽出、33-bit extension、monotonic 補間だけで足りるため、既存 soft_demux 内の小さな自前実装を維持する方がよい。

### 6.4 libdvbpsi

libdvbpsi は MPEG TS / DVB PSI の decode / generation library で、PSI/SI 解析には有用である。ただし、PCR→local wallclock clock discipline の as-is 部品ではない。

## 7. r50z / r51 方針

r50z では、以下を採用する。

```text
- PCR を source clock の primary source にする。
- PCR 未観測時は valid A/V sync ID を返さない。
- valid A/V sync ID を返した場合は getAvSyncTime(id) が valid timestamp を返す。
- PTS を current A/V sync clock の代替にしない。
- 最小 wallclock 補間を行う。
- 33-bit wrap は extended 90kHz 値へ伸長する。
- linear regression / PLL / drift compensation は r51 以降の品質改善候補とする。
```

r51 以降で追加検討すべきことは以下である。

```text
- PCR PID を PMT から特定し、同一 service の PCR だけを clock source とする。
- PCR jitter を平滑化する。
- local monotonic clock と PCR clock の drift を線形回帰または PLL で推定する。
- 長時間視聴時の drift / wrap / discontinuity を評価する。
- discontinuity indicator や service 切替時に clock state を reset する。
```

## 8. 参照 URL

- AOSP HIDL `IDemux.getAvSyncTime()` 仕様: https://android.googlesource.com/platform/hardware/interfaces/+/7239943cb5fcac4440a45a7bc5ce75c189d6df76/tv/tuner/1.0/IDemux.hal
- AOSP Java `Tuner.getAvSyncTime()` 仕様: https://android.googlesource.com/platform/prebuilts/fullsdk/sources/android-30/+/refs/heads/androidx-camera-release/android/media/tv/tuner/Tuner.java
- AOSP CTS `TunerTest.testAvSyncId()`: https://android.googlesource.com/platform/cts/+/bc74b1feb21/tests/tests/tv/src/android/media/tv/tuner/cts/TunerTest.java
- AOSP AIDL VTS `DemuxTests`: https://android.googlesource.com/platform/hardware/interfaces/+/613782fa78a0ee7a97b5c7de6febce089f152fa8/tv/tuner/aidl/vts/functional/DemuxTests.cpp
- AOSP AIDL VTS target test: https://android.googlesource.com/platform/hardware/interfaces/+/613782fa78a0ee7a97b5c7de6febce089f152fa8/tv/tuner/aidl/vts/functional/VtsHalTvTunerTargetTest.cpp
- HIDL default/VTS A/V sync 追加 commit: https://gerrit.omnirom.org/plugins/gitiles/android_hardware_interfaces/+/b717eb547ee9da1cf5b83e02ba3a3d8161d949db%5E%21/
- GStreamer `mpegtslivesrc` 解説: https://centricular.com/devlog/2024-12/mpegtslivesrc/
- GStreamer Rust plugins README: https://github.com/GStreamer/gst-plugins-rs
- GStreamer Rust plugins changelog: https://github.com/GStreamer/gst-plugins-rs/blob/main/CHANGELOG.md
- Rust `mpegts` crate docs: https://docs.rs/mpegts/latest/mpegts/parser/index.html
- VideoLAN libdvbpsi: https://www.videolan.org/developers/libdvbpsi.html
