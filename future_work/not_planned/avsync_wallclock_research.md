# A/V sync wallclock 補間調査と非採用範囲

## 位置付け

この文書は、A/V sync のうち本製品で採用しない高度な clock discipline と、その判断に使った調査結果を記録する未解決条件資料である。現行設計判断、戻り値、状態遷移、対応宣言条件の正本ではない。現行の `getAvSyncHwId()` / `getAvSyncTime()` 契約は `tuner_contract/DESIGN_JA.md` を正とする。

この文書に記載する先行例、外部OSS、調査URLは、再検討時に参照するための根拠情報であり、そのまま製品へ組み込む計画または対応宣言を意味しない。

## AOSP 側の期待仕様

AOSP HIDL `IDemux` の仕様では、`getAvSyncTime()` は A/V sync に使う current timestamp を返し、hardware が current timestamp を increment / maintain する、と説明されている。timestamp は 90kHz base で、PTS と同じ format である。

AOSP Java `Tuner.getAvSyncTime()` も、timestamp は hardware によって maintain され、90kHz base で PTS と同じ format と説明している。

ここでいう PTS は format の説明であり、PES PTS を current A/V sync clock の代替として返してよいという意味ではない。software demux が current clock をまだ持たない段階では、valid A/V sync ID を返さない設計が安全である。

## wallclock 補間とは何か

wallclock 補間は、最後に観測した PCR と、その PCR を観測したローカル monotonic clock 時刻を対応付け、その後の経過時間を 90kHz tick に換算して足す処理である。

```text
estimated_90khz = last_pcr_90khz + elapsed_monotonic_ns * 90000 / 1_000_000_000
```

現行設計では、PCR 由来の source clock が存在しない段階で valid A/V sync ID を先出ししない。valid A/V sync ID を返す場合は、対応する `getAvSyncTime(id)` が有効 timestamp を返せる状態に限る。この現行契約は `tuner_contract/DESIGN_JA.md` の A/V sync 節を正とし、本ファイルでは再定義しない。

## 先行例

### GStreamer `mpegtslivesrc`

GStreamer の `mpegtslivesrc` は、MPEG-TS live source を wrap し、stream の PCR に基づく clock を提供する element である。GStreamer Rust plugins の README でも、`mpegtslivesrc` は `udpsrc` や `srtsrc` などの MPEG-TS source を wrap し、stream の PCR に基づく live clock を提供すると説明されている。

Centricular の開発記事では、`mpegtslivesrc` が in-stream PCR を使って sender clock time と local receive time を相関させ、linear regression で sender clock と local system clock の相対 rate を計算すると説明している。

これは現行の最小補間より高度である。本製品の現行設計は「最後の PCR + monotonic 経過時間」だけを扱い、linear regression による drift 推定は採用しない。

### GStreamer `mpegtsdemux` / `MpegTSBase`

GStreamer の MPEG-TS demux 系には PCR を timing に使う設計がある。`ignore-pcr` という property も存在し、PCR が timing の入力として扱われていることが分かる。

ただし、GStreamer の一般的な同期設計文書では、stream 内容から clock を生成することは可能だが推奨されない場面もある、とされている。stream 由来 clock は jitter や破損 stream の影響を受けやすいためである。AOSP が current A/V sync timestamp を要求する以上、PCR 由来 clock の最小実装には合理性があるが、長時間運用では drift correction の余地がある。

### AOSP / ExoPlayer / Media3 の timestamp adjuster

ExoPlayer / Media3 系には MPEG-TS の 33-bit timestamp wrap を扱う timestamp adjuster がある。これは PTS/PCR 系 timestamp の wrap と offset 調整を扱うもので、HAL の current A/V sync clock provider としてそのまま利用するものではない。

### FFmpeg 系

FFmpeg は MPEG-TS demux / mux で PCR/PTS/DTS を扱うが、今回調査範囲で「Android Tuner HAL の `getAvSyncTime()` にそのまま流用できる PCR→local wallclock clock discipline 部品」は確認できない。

## AOSP 側に wallclock 補間の参照実装があるか

今回の用途にそのまま使える AOSP 参照実装は確認できない。

確認できたものは以下である。

- `Tuner.getAvSyncHwId(Filter)`
- `Tuner.getAvSyncTime(int)`
- HIDL `IDemux.getAvSyncTime()`
- JNI bridge
- CTS test

これらは期待仕様と API 挙動を示すが、PCR を wallclock 補間して current 90kHz timestamp を作る実装ではない。

古い HIDL default 実装では、PCR filter ID を A/V sync ID として扱うテスト用実装の痕跡がある。ただし、この default 実装は実用的な wallclock 補間実装ではない。少なくとも今回の Android 14 AIDL HAL にそのまま使う clock discipline ではない。

## as-is で組み込み利用可能な OSS があるか

今回の HAL に as-is で組み込むべき OSS はない。

### GStreamer `mpegtslivesrc`

`mpegtslivesrc` は最も近い先例である。PCR と local receive time を対応付け、linear regression で sender clock を local GStreamer clock として近似する。

しかし、これは GStreamer plugin と GStreamer clock infrastructure に依存する。Android vendor Tuner HAL の Rust service に as-is で組み込むには、GStreamer runtime、plugin registry、GObject/GStreamer threading model、ライセンス・ビルド統合を持ち込む必要がある。Tuner HAL に直接組み込むには重すぎる。

採用可能なのはアルゴリズム上の参考である。

### ExoPlayer / Media3 `TimestampAdjuster`

Apache-2.0 であり Android 親和性は高いが、これは Java/Kotlin 側の media extractor 向け timestamp 調整部品である。HAL 内部の Rust demux に as-is 組み込みするには向かない。

採用可能なのは 33-bit wrap handling と offset adjustment の考え方である。

### Rust MPEG-TS parser crates

`mpegts` や `mpeg2ts-reader` のような Rust crate は、MPEG-TS packet / adaptation field / PCR parse の参考にはなる。ただし、Android platform build / Soong / vendor HAL へ外部 crate を追加するコスト、保守性、既存 soft_demux との重複を考えると、as-is 採用する利点は小さい。

現行実装に必要なのは PCR 6 byte の抽出、33-bit extension、monotonic 補間であり、既存 soft_demux 内の小さな自前実装を維持する方がよい。

### libdvbpsi

libdvbpsi は MPEG TS / DVB PSI の decode / generation library で、PSI/SI 解析には有用である。ただし、PCR→local wallclock clock discipline の as-is 部品ではない。

## 本ファイルで扱う非採用範囲

次は本製品の現行対応宣言・実装済み範囲に含めない。

```text
- PCR jitter smoothing
- linear regression による clock drift 推定
- PLL / clock discipline
- 複数 clock source の品質評価
- 長時間視聴時の drift 補正
- discontinuity indicator と clock reset を組み合わせた高度な service clock model
```

## 非採用理由

AOSP Tuner HAL の `getAvSyncTime()` は current A/V sync timestamp を要求するが、上記の高度な clock discipline は非スクランブル平文ライブ視聴、VTS 接続確認、最小 A/V sync 契約を成立させるための必須条件ではない。

これらを実装済み扱いにする場合は、`tuner_contract/DESIGN_JA.md` に clock source、PCR PID、jitter、drift、reset 条件、戻り値、診断、実機確認条件を吸収してから扱う。本ファイルを根拠に現行リリースで対応宣言してはならない。

## 参照 URL

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
