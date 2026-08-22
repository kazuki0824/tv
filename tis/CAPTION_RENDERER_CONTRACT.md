# ARIB 字幕 renderer 統合契約

この文書は、r51 の ARIB 字幕表示について `libaribcaption` の build/link、renderer viewport、PTS、native lifecycle を固定する設計正本である。

`tis/DESIGN_JA.md` の「libaribcaption Soong / renderer 統合境界」「字幕PTS scheduling / clear ownership」と `tis/INTEGRATION.md` の「libaribcaption Soong / renderer 統合」に、この文書と矛盾する旧 shared-library 前提が残る場合は本書を正とする。実装完了時には旧記述を本書へ同期し、future_work 側の旧 shared-library 完了条件を削除する。

対象の `libaribcaption-android` は、製品で pin した fork を repo で source tree 内へ配置し、同 fork の Soong module を build graph 内で使用する。out-of-graph prebuilt、実行時探索、renderer 無効 build は正式経路にしない。

## 1. static link を正式経路とする

正式な依存関係は次に固定する。

```text
/system_ext/priv-app/MaleicacidTvInput
  └─ libmaleicacid_arib_caption_jni.so
       └─ Soong static dependency: libaribcaption
            └─ static dependency: libft2.nodep
```

`libaribcaption-android` 側は `cc_library_static { name: "libaribcaption" }` を正とする。`libmaleicacid_arib_caption_jni` は `system_ext_specific: true` のまま、Soong の静的 native dependency として `libaribcaption` を直接 link する。Rust JNI から libaribcaption C API を `extern "C"` で直接参照し、`libdl`、`dlopen()`、`dlsym()`、`dlclose()` を正式経路から除去する。

`libaribcaption.so` は生成・配置・APK同梱を要求しない。したがって次を完了条件にしてはならない。

```text
- libaribcaption.so の存在
- libmaleicacid_arib_caption_jni.so の DT_NEEDED に libaribcaption.so が出ること
- MaleicacidTvInput の jni_libs に libaribcaption を追加すること
- libaribcaption.so の export symbol を readelf で確認すること
```

代わりに、対象 LineageOS/Soong tree で次を完了条件とする。

```text
- m libaribcaption が通る。
- m libmaleicacid_arib_caption_jni が通る。
- libmaleicacid_arib_caption_jni.so の undefined native symbol に未解決の aribcc_* が残らない。
- libmaleicacid_arib_caption_jni.so に libaribcaption.so への DT_NEEDED が存在しない。
- runtime で dlopen/dlsym を使用しない。
- renderer C API を実際に呼ぶ。
```

Soong の静的依存指定は、対象 tree で Rust `rust_ffi_shared` から C/C++ static module を正しく最終 link できる形を選ぶ。`static_libs` と `whole_static_libs` のどちらを使うかは link graph と dead-strip 要件に従う実装詳細だが、libaribcaption の C API とその `libft2.nodep` 依存が最終 JNI `.so` で全て解決されることを build gate で確認する。不要な全 archive 強制取り込みを設計目的にはしない。

## 2. renderer viewport / 座標契約

libaribcaption renderer は render 前に `aribcc_renderer_set_frame_size()` を必ず呼ぶ。固定 1920x1080、字幕 plane size、端末 display sizeを代替値として推測使用してはならない。

TIS は playback generation ごとに `CaptionViewport` を一つ所有する。`CaptionViewport` は、現在の session Surface に対応して実際に字幕を重ねる video content viewport を TIS overlay 座標系で表す。

```text
CaptionViewport:
  overlayWidthPx   > 0
  overlayHeightPx  > 0
  contentLeftPx
  contentTopPx
  contentWidthPx   > 0
  contentHeightPx  > 0
  generationToken
```

`contentLeftPx/contentTopPx/contentWidthPx/contentHeightPx` は letterbox / pillarbox を含む overlay 全体ではなく、current video content を表示する矩形を表す。video を持たない audio-only service では字幕 renderer viewport を成立させず、映像座標への字幕表示を成功扱いしない。

renderer の frame size は `contentWidthPx x contentHeightPx` に設定する。libaribcaption が返す `dst_x/dst_y/width/height` はこの renderer frame 左上を原点とする座標として扱い、Kotlin overlay では `contentLeftPx/contentTopPx` を一度だけ加算して配置する。別の独自 scale、ARIB plane からの再計算、Canvas text layout は追加しない。

RGBA8888 image は Rust が libaribcaption 所有領域から Rust-owned buffer へ copy した後に cleanup し、JNI を越えた後は Kotlin/Bitmap 側が所有する。stride を無視して `width * 4` の密な配列と仮定してはならない。Bitmap 生成時は width/height/stride と buffer size の整合を境界検査する。

viewport が未確定、幅/高さが0、generation不一致の場合は `aribcc_renderer_set_frame_size()` / render へ進まず、字幕表示成功にしない。viewport変更時は serial subtitle executor 上で旧 scheduled event を cancelし、旧 overlay bitmap を clear する。current native renderer が同一 generation で安全に frame size を更新できる場合は新 frame size を設定し current media time で再 render する。再 render できない場合は旧 bitmap を拡大縮小して流用せず、次の有効 caption まで clear を維持する。

## 3. 字幕 PTS / NoPTS 契約

字幕表示に使用する時間軸は current playback generation の MediaSync canonical media time だけとする。PCR、wallclock、受信時刻、固定 delay、直前 caption PTS、nominal frame rate から字幕 PTS を生成しない。

字幕 PES に authoritative な 33-bit 90 kHz PTS がある場合だけ、current playback generation の PTS unwrap 規則で canonical `timeUs` へ変換し、その時刻を decoder/renderer/scheduler の同一 caption 時刻として使用する。

libaribcaption の `ARIBCC_PTS_NOPTS` / `PTS_NOPTS` を renderer へ append しない。入力 caption に renderer が使用できる authoritative PTS がない場合は次に固定する。

```text
- 0 に丸めない。
- 直前 PTS を carry-forward しない。
- PCR / wallclock / MediaSync current position を caption PTS として捏造しない。
- renderer queue に append しない。
- その caption を表示成功として扱わない。
- 型付き診断へ記録する。
- 既に表示中の有効 caption は、その caption 自身の duration / clear / lifecycle 契約に従い維持し、NoPTS 入力だけを理由に即時 clear しない。
```

現行製品profileが字幕表示対応を宣言するためには、字幕 filter / producer が表示対象 caption PES について renderer に渡せる authoritative PTS を供給できることを qualification 条件に含める。これを満たせない backend/profile は、decoder が文字列を抽出できても r51 字幕表示成功対応として表明しない。

有限 duration は `caption PTS + duration` を同じ canonical timeline 上の clear 境界とする。`DURATION_INDEFINITE` は推測で有限化しない。scheduler は `MediaSync.getTimestamp()` を event 到達確認に使う one-shot scheduling とし、周期 polling や独立 clock を作らない。

## 4. decoder / renderer / scheduler lifecycle

字幕 native state、scheduler state、overlay state は同じ subtitle generation に属する。少なくとも次を一組として扱う。

```text
SubtitleGeneration:
  playbackGenerationToken
  selectedSubtitleTrackId
  CaptionViewport
  libaribcaption context
  decoder
  renderer
  pending one-shot event
  current rendered frame
```

状態変更は session/subtitle の serial executor 上に直列化し、旧 generation callback/result は generation token 不一致で破棄する。

### enable / select

字幕が enabled かつ subtitle track が選択され、current playback generation と有効 viewport が揃った場合だけ native renderer path を active にする。新しい subtitle generation を開始するときは context/decoder/renderer を既知の初期状態から構築し、renderer initialize 後に current viewport で `aribcc_renderer_set_frame_size()` を成功させてから caption input を受け入れる。

### disable

`onSetCaptionEnabled(false)` は pending scheduler event を cancelし、overlay を即時 clear し、`aribcc_renderer_flush()` 相当で renderer queue / current render state を失効させる。disabled 中の PES を表示用 renderer queue に蓄積しない。

再 enable 時に、disable 中に停止・flush された subtitle filter の continuity を仮定しない。native decoder/renderer state の継続可否が証明できない場合は新しい subtitle generation として再初期化する。実装は「古い decoder state を暗黙再利用する」より再初期化を既定とする。

### track deselect / track change

`onSelectTrack(TYPE_SUBTITLE, null)` は即時に scheduler cancel、overlay clear、renderer flush を行い、current subtitle generation を終了する。別 subtitle track への変更も同様に旧 generation を終了し、新 track 用 context/decoder/renderer を新規初期化する。旧 track の caption/result/event を新 track に持ち越さない。

### subtitle filter flush / restart

字幕 filter 自身の flush、stop/reconfigure/restart により data-group continuity が失われ得る場合は、pending scheduler event と overlay を clear し、renderer を flush し、decoder/renderer を新 subtitle generation として再初期化する。A/V filter だけの plain flush は字幕 generation を変更しない。

### retune / playback generation change / Surface change

物理 retune、service/codec/PID graph 変更、playback generation変更、Surface/MediaSync generation変更では旧 subtitle generation を終了する。pending event cancel、overlay clear、renderer flush、decoder/renderer/context 解放を行い、新 playback generation では新しい viewport と timing epoch が確定するまで字幕 input を表示成功にしない。

### viewport change

同じ playback generation 内の純粋な viewport size/position変更では decoder continuity を壊さない。schedulerを一旦止め旧 bitmap を clearし、renderer frame size を新 `contentWidthPx/contentHeightPx` へ更新する。current media time で安全に再 render できる場合だけ新 viewport に再表示し、できない場合は次の caption まで clear を維持する。

### session release

session release は pending event cancel、overlay clear、renderer flush の後、renderer → decoder → context の依存関係を壊さない順で解放し、subtitle executor 上の queued stale work を released flag/generation token で破棄する。release 後に native callback/result が UI state を変更してはならない。

## JNI 出力 model / 所有権

Rust JNI の表示用出力は文字列ではなく renderer 結果を表す。

```rust
struct RenderedCaptionFrame {
    pts_millis: i64,
    duration_millis: Option<i64>,
    images: Vec<RenderedCaptionImage>,
}

struct RenderedCaptionImage {
    dst_x: i32,
    dst_y: i32,
    width: i32,
    height: i32,
    stride: i32,
    rgba8888: Vec<u8>,
}
```

libaribcaption の caption/result/image の cleanup は Rust FFI 境界で完結させる。Kotlinへ libaribcaption の raw pointer や借用寿命を漏らさない。Kotlin `CaptionOverlayView` は renderer image を Bitmap 化して viewport origin を加算して `drawBitmap()` するだけとし、`caption.text` / `Canvas.drawText()` を字幕表示正式経路に残さない。

## 最低検証ゲート

```text
Build:
- m libaribcaption
- m libmaleicacid_arib_caption_jni
- m MaleicacidTvInput
- static link が解決し、libaribcaption.so DT_NEEDED を要求しない

Renderer:
- set_frame_size 前の render を成功扱いしない
- valid viewport で RGBA8888 + dst rect が overlay に出る
- stride / buffer size を検査する
- viewport change で旧 bitmap 座標を流用しない

PTS:
- valid PTS caption は canonical timeline に表示される
- NoPTS を 0/前値/PCR/wallclock で補完しない
- NoPTS caption は renderer append / display success にしない
- NoPTS input が既存有効 caption を根拠なく即時 clear しない

Lifecycle:
- disable で scheduler cancel + overlay clear + renderer flush
- deselect で同上 + subtitle generation 終了
- track change で旧 result を破棄し新 native stateを作る
- subtitle filter continuity loss で native state を reset
- A/V-only plain flush では字幕 generation をresetしない
- retune / playback generation / Surface generation change で旧字幕を破棄
- release 後の stale callback/result が描画しない
```

これらを満たした後にのみ、`future_work/r51/libaribcaption_android_soong_ready_plan(1).md` の shared-library 前提を static-link 前提へ同期し、実装・実機検証が通った時点で future_work から削除する。