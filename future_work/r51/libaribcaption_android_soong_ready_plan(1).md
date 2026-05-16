# libaribcaption-android Soong-ready 化作業計画

作成日: 2026-05-16  
対象: `https://github.com/kazuki0824/libaribcaption-android/tree/master`  
前提: `libaribcaption-android` は repo コマンドで供給される外部プロジェクトとして扱う。

## 1. 結論

この作業計画は技術的に成立する。

ただし、成立するのは次の形に固定した場合である。

```text
正式方針:
  repo で供給される libaribcaption-android の製品 fork に Android.bp を追加し、
  AOSP/Soong の build graph 内で renderer 有効の libaribcaption.so を生成する。

禁止する完了扱い:
  - libaribcaption.so を dlopen できるだけで完了扱いにすること。
  - decoder API だけを呼んで文字列を Canvas.drawText() で描くこと。
  - renderer 無効ビルドを字幕表示完了扱いにすること。
  - AOSP build graph 外で作った .so を、出所・オプション不明のまま同梱すること。
```

今回の作業の主対象は `libaribcaption-android` 側の Soong 対応である。ただし、本プロジェクトの r51 字幕表示完了条件を満たすには、TIS 側の Rust JNI と Kotlin overlay の改修も同じ完了条件に含める必要がある。

## 2. 成立性判断

| 観点 | 判断 | 理由 |
|---|---|---|
| Soong 直ビルド | 成立する | upstream は C++17 のライブラリで、CMake の source list と compile definition を Soong の `cc_library_shared` へ移せる。 |
| renderer 有効化 | 成立する | CMake 上も renderer は既定で有効で、`ARIBCC_NO_RENDERER` を指定すると無効化される構造である。Android では FreeType が必要になる。 |
| FreeType 結線 | 成立する | AOSP 側の Android target 用 FreeType module を `shared_libs` または `static_libs` で参照すればよい。Ubuntu host の `libfreetype-dev` は使わない。 |
| TIS からの利用 | 成立する | `libmaleicacid_arib_caption_jni` が `libaribcaption` に明示依存し、`MaleicacidTvInput` の `jni_libs` に同梱すれば、実行時の所在が固定される。 |
| renderer 出力表示 | 成立する | libaribcaption の C API は renderer 生成、caption 追加、PTS 指定描画、描画結果解放、RGBA8888 画像を提供している。 |
| C/C++ 薄層禁止との整合 | 成立する | 独自 C/C++ 薄層を書かず、Rust FFI から libaribcaption C API を直接呼ぶ構造にする。 |
| リスク | 中 | CMake の条件付き source list、生成 header、FreeType include/link、export symbol、x86 SSE option の Soong 移植は実ビルドで調整が必要。 |

## 3. 根拠

### 3.1 libaribcaption-android 側

`libaribcaption-android` は ARIB STD-B24 字幕の decoder / renderer であり、C API も提供する。Android は対応 platform に含まれ、Android では FreeType が必要である。

CMake 上は `ARIBCC_NO_RENDERER` で renderer を無効化できる。つまり、Soong 側ではこの定義を入れないことが renderer 有効化の基本条件である。

また、Android 判定時は `ARIBCC_IS_ANDROID` を有効化し、`ARIBCC_USE_FREETYPE` を有効にする構造である。したがって、Soong 化では Android 用 config header でこれを固定する。

### 3.2 本プロジェクト側

本プロジェクトでは、r51 の字幕完了条件が次に固定されている。

```text
- PMT から字幕トラックを検出する。
- TvTrackInfo.TYPE_SUBTITLE として通知する。
- onSetCaptionEnabled() に応答する。
- onSelectTrack(TYPE_SUBTITLE, trackId) に応答する。
- 字幕 PES を libaribcaption C API 経路へ渡す。
- libaribcaption レンダラー API で描画結果を得る。
- TIS overlay 上に描画結果を表示する。
- 字幕無効化、トラック解除、セッション解放時に表示を消す。
- libaribcaption が使えない場合や描画に失敗した場合、字幕対応済みとして成功扱いしない。
```

ここで重要なのは、AOSP/TIS 契約単体は `libaribcaption` という実装名を要求しないが、本プロジェクトでは ARIB 字幕本文処理を TIS 側の `libaribcaption` 経路へ閉じると固定している点である。したがって r51 の字幕表示完了条件は、AOSP/TIS の字幕 track 契約を満たすことに加え、`libaribcaption` のレンダラーが生成した描画結果を実際に表示することで満たす。

### 3.3 decoder-only が r51 未達である理由

過去整理で `dlopen("libaribcaption.so")` と decoder API 呼び出しだけでは r51 完了条件に達しないと判断したロジックは、次の因果関係で固定する。

| 判定対象 | r51 完了条件との関係 | 判定 | 理由 |
|---|---|---|---|
| `dlopen("libaribcaption.so")` | ライブラリを実行時に開けることだけを示す | 未達 | `.so` の所在確認であり、renderer が有効に build されていること、renderer symbol が存在すること、renderer API を呼んでいることを保証しない。 |
| decoder API 呼び出し | 字幕 PES を libaribcaption に渡す入口の一部である | 未達 | decoder は字幕文・字幕構造の解析段階であり、ARIB 字幕の位置、色、サイズ、DRCS/外字、組版を反映した描画結果を返す段階ではない。 |
| `caption.text` などの文字列取得 | 簡易的な文字列抽出にすぎない | 未達 | r51 が要求するのは字幕文字列の抽出ではなく、libaribcaption レンダラーの描画結果を TIS overlay に表示することである。 |
| Kotlin の `Canvas.drawText()` | Android Canvas に文字列を直接描く独自簡易表示である | 未達 | libaribcaption renderer が計算した描画面、領域、座標、色、文字サイズ、DRCS/外字、組版結果を表示していない。 |
| renderer API 呼び出し | `aribcc_renderer_append_caption()` と `aribcc_renderer_render()` で描画結果を得る | 必須 | decoder 結果を renderer へ投入し、PTS 指定で RGBA8888 画像と座標を得て、それを overlay に合成する必要がある。 |
| renderer 出力画像の overlay 合成 | TIS が実表示まで責任を持つ | 必須 | `TvTrackInfo` 通知、track 選択、`onSetCaptionEnabled()` と、実際の字幕表示状態を一致させる必要がある。 |

このため、r51 では次をすべて満たす必要がある。

```text
必須:
  - libaribcaption.so が Soong build graph 内で生成される。
  - ARIBCC_NO_RENDERER が定義されていない。
  - libaribcaption.so が renderer C API symbol を export する。
  - Rust JNI が decoder API だけでなく renderer API を呼ぶ。
  - Rust JNI が renderer 出力画像を安全に所有権変換して Kotlin へ返す。
  - Kotlin overlay が renderer 出力画像を bitmap として合成する。

禁止:
  - dlopen 成功だけを字幕表示対応とみなす。
  - decoder API 成功だけを字幕表示対応とみなす。
  - caption.text を Canvas.drawText() で描いて r51 字幕表示完了とみなす。
  - renderer 無効 build の libaribcaption.so を受け入れる。
```

### 3.4 Soong-ready 化への反映

上記の判定により、Soong-ready 化の作業範囲は `Android.bp` 追加だけでは閉じない。`libaribcaption-android` 側では renderer 有効の `libaribcaption.so` を生成し、本プロジェクト側ではその renderer API を実際に使用する必要がある。

したがって、以後の作業項目では次を一体の完了条件として扱う。

```text
- libaribcaption 側: renderer 有効の Soong module を作る。
- Rust JNI 側: decoder-only から renderer 呼び出しへ変更する。
- Kotlin overlay 側: 文字列描画から renderer 出力画像の合成へ変更する。
- 検証側: renderer symbol、明示 link、画像合成、字幕無効化時の消去を確認する。
```

## 4. repo manifest 方針

`libaribcaption-android` は製品側 fork として固定する。

```xml
<project
    name="<your-org>/libaribcaption-android"
    path="vendor/maleicacid/tv/tis/third_party/libaribcaption-android"
    revision="<pinned-commit>"
    remote="<your-remote>" />
```

### 4.1 直接 upstream を参照しない理由

upstream へ直接 `Android.bp` を置けないため、製品 fork を作る。fork には次だけを持たせる。

```text
- Android.bp
- Soong 用 aribcc_config.h または生成規則
- 必要なら Soong 用 README / BUILD_INFO
- upstream 追従時の最小補正
```

### 4.2 revision 固定

`revision` は branch ではなく commit 固定にする。字幕表示は r51 完了条件に直結するため、repo sync のたびに source list や C API が変わる状態を避ける。

## 5. libaribcaption-android 側の改変計画

### WP-01: Soong package 化

`vendor/maleicacid/tv/tis/third_party/libaribcaption-android/Android.bp` を追加する。

最低限必要な module は次である。

```bp
package {
    default_applicable_licenses: ["libaribcaption_android_license"],
}

license {
    name: "libaribcaption_android_license",
    visibility: [":__subpackages__"],
    license_kinds: ["SPDX-license-identifier-MIT"],
    license_text: ["LICENSE"],
}

cc_library_shared {
    name: "libaribcaption",
    product_specific: true,
    min_sdk_version: "31",

    cpp_std: "c++17",
    stl: "libc++",

    srcs: [
        // CMakeLists.txt の decoder/base source
        // CMakeLists.txt の renderer source
        // Android + FreeType に必要な source
    ],

    local_include_dirs: [
        "include",
        "src",
    ],

    generated_headers: [
        "libaribcaption_android_config_headers",
    ],
    export_include_dirs: [
        "include",
    ],
    export_generated_headers: [
        "libaribcaption_android_config_headers",
    ],

    cflags: [
        "-DARIBCC_IMPLEMENTATION",
    ],

    shared_libs: [
        "libft2",
    ],
}
```

`shared_libs: ["libft2"]` は初期案である。対象 AOSP tree で FreeType module の vendor/product variant が合わない場合は、`static_libs` 化または module variant 調整を行う。

### WP-02: Android 用 config header を固定する

CMake の `aribcc_config.h.in` 相当を Soong で供給する。

推奨は、生成規則ではなく Soong 用 header を明示配置する方式である。

```text
soong/include/aribcaption/aribcc_config.h
```

内容方針:

```c
#ifndef ARIBCAPTION_ARIBCC_CONFIG_H
#define ARIBCAPTION_ARIBCC_CONFIG_H

#define ARIBCC_SHARED_LIBRARY 1
#define ARIBCC_IS_ANDROID 1
#define ARIBCC_USE_FREETYPE 1

/* ARIBCC_NO_RENDERER は定義しない。 */
/* ARIBCC_USE_FONTCONFIG / CORETEXT / DIRECTWRITE / GDI は定義しない。 */

#endif
```

`genrule` で `.in` から生成してもよいが、CMake の `#cmakedefine` 互換処理を shell で再現すると保守性が落ちる。r51 の安定性を優先するなら、Soong 用 header を固定ファイルとして管理する。

### WP-03: source list を CMake から Soong へ移す

`CMakeLists.txt` の source list を次の分類で移す。

```text
必須:
  - decoder / context / caption / parser / base 系 source
  - renderer 共通 source
  - image C API source
  - renderer C API source
  - FreeType text renderer source
  - Android で必要な tinyxml2 source

除外:
  - DirectWrite
  - CoreText
  - GDI
  - Fontconfig
  - test source
  - sample source
  - embedded FreeType fetch logic
```

x86/x86_64 向け SSE/SSE2 option は CMake と同じく対象 arch のみで有効化する。Soong では `target.android_x86.cflags` / `target.android_x86_64.cflags` で分岐する。

### WP-04: export header と symbol を確認する

`aribcc_export.h` と `ARIBCC_SHARED_LIBRARY` の組み合わせで C API symbol が `.so` から export されることを確認する。

完了条件:

```bash
m libaribcaption
readelf -Ws out/.../libaribcaption.so | grep ' aribcc_renderer_render$'
readelf -Ws out/.../libaribcaption.so | grep ' aribcc_renderer_append_caption$'
readelf -Ws out/.../libaribcaption.so | grep ' aribcc_render_result_cleanup$'
```

## 6. 本プロジェクト側の改変計画

### WP-05: Rust JNI を明示 link に変更する

`vendor/maleicacid/tv/tis/arib_caption_jni/Android.bp` を変更する。

現行の `libdl` 依存と `dlopen("libaribcaption.so")` 前提は廃止する。

```bp
rust_ffi_shared {
    name: "libmaleicacid_arib_caption_jni",
    ...
    shared_libs: [
        "libaribcaption",
    ],
}
```

完了条件:

```bash
m libmaleicacid_arib_caption_jni
readelf -d out/.../libmaleicacid_arib_caption_jni.so | grep 'NEEDED.*libaribcaption.so'
```

### WP-06: APK へ同梱する

`vendor/maleicacid/tv/tis/Android.bp` の `MaleicacidTvInput` に `libaribcaption` を追加する。

```bp
jni_libs: [
    "libmaleicacid_arib_si_engine_jni",
    "libmaleicacid_arib_caption_jni",
    "libaribcaption",
],
```

完了条件:

```bash
m MaleicacidTvInput
```

APK または product priv-app 配置に、少なくとも次が同梱されることを確認する。

```text
libmaleicacid_arib_caption_jni.so
libaribcaption.so
```

### WP-07: Rust JNI を renderer API 経路へ変更する

Rust JNI の責務を decoder-only から renderer 経路へ変更する。

最低限呼ぶ C API:

```text
- aribcc_context_alloc / aribcc_context_free
- aribcc_decoder_alloc / aribcc_decoder_free
- aribcc_decoder_initialize
- aribcc_decoder_set_profile
- aribcc_decoder_set_caption_type
- aribcc_decoder_decode
- aribcc_caption_cleanup
- aribcc_renderer_alloc / aribcc_renderer_free
- aribcc_renderer_initialize
- aribcc_renderer_append_caption
- aribcc_renderer_render
- aribcc_renderer_flush
- aribcc_render_result_cleanup
```

Rust 側の内部 model:

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

所有権規則:

```text
- libaribcaption から返された bitmap は Rust Vec へコピーしてから cleanup する。
- aribcc_caption_cleanup は decode 成功後に必ず呼ぶ。
- aribcc_render_result_cleanup は render result を Rust model に変換した後に必ず呼ぶ。
- renderer / decoder / context の解放順を固定する。
```

### WP-08: Kotlin 側を描画結果 model へ変更する

`NativeAribCaptionRenderer.DecodedCaption(text, ptsMillis)` を廃止し、描画結果を受ける model に変更する。

例:

```kotlin
data class RenderedCaptionFrame(
    val ptsMillis: Long,
    val durationMillis: Long?,
    val images: List<RenderedCaptionImage>,
)

data class RenderedCaptionImage(
    val dstX: Int,
    val dstY: Int,
    val width: Int,
    val height: Int,
    val stride: Int,
    val rgba8888: ByteArray,
)
```

`CaptionOverlayView` は `Canvas.drawText()` ではなく、RGBA8888 bitmap を `Bitmap` 化して `drawBitmap()` で合成する。

完了条件:

```text
- 字幕有効時だけ表示する。
- subtitle track 未選択時は表示しない。
- onSetCaptionEnabled(false) で表示を消す。
- session release で renderer flush と overlay clear を行う。
- render 失敗時に字幕成功扱いにしない。
```

## 7. 検証計画

### 7.1 build 検証

```bash
m nothing
m libaribcaption
m libmaleicacid_arib_caption_jni
m MaleicacidTvInput
```

### 7.2 symbol / link 検証

```bash
readelf -Ws out/.../libaribcaption.so | grep ' aribcc_renderer_render$'
readelf -d out/.../libmaleicacid_arib_caption_jni.so | grep 'NEEDED.*libaribcaption.so'
```

### 7.3 r51 字幕契約検証

```text
1. PMT から字幕 component を検出する。
2. TvTrackInfo.TYPE_SUBTITLE を notifyTracksChanged() で通知する。
3. onSelectTrack(TYPE_SUBTITLE, trackId) に応答する。
4. onSetCaptionEnabled(true) 後に ARIB 字幕 PES を renderer へ渡す。
5. renderer 出力 bitmap を overlay に表示する。
6. onSetCaptionEnabled(false) / track 解除 / session release で表示を消す。
7. libaribcaption 初期化失敗、decode 失敗、render 失敗を成功扱いにしない。
```

### 7.4 負試験

```text
- ARIBCC_NO_RENDERER を誤って定義した場合、CI または build-time check で失敗させる。
- libaribcaption.so を jni_libs から外すと MaleicacidTvInput の検証で失敗する。
- renderer symbol が export されない場合、readelf 検証で失敗する。
- Kotlin 側が文字列描画経路へ戻った場合、受け入れテストで失敗する。
```

## 8. 残る注意点

### 8.1 Soong source list は機械移植しない

CMake の generator expression をそのまま機械変換しない。Android + FreeType + renderer 有効という今回の固定条件に合わせ、必要 source を明示列挙する。

### 8.2 `dlopen` 継続は避ける

Soong-ready 化する目的は、依存関係を build graph に載せることである。`dlopen` を残すと、renderer 無効の `.so` や不在の `.so` を実行時まで隠すため、正式構成として弱い。

### 8.3 prebuilt 案は代替案に落とす

どうしても Soong 直ビルドが短期で詰まる場合だけ、Android NDK + CMake で renderer 有効 `.so` を作り、`cc_prebuilt_library_shared` で取り込む。ただし正式方針ではない。prebuilt 案を使う場合は `BUILD_INFO.md` に commit、NDK version、CMake option、ABI、sha256、FreeType の取得元を記録する。

## 9. 最終完了条件

この作業は次を全て満たした時だけ完了扱いにする。

```text
- repo sync 後に libaribcaption-android が固定 path に展開される。
- `m libaribcaption` が通る。
- `libaribcaption.so` に renderer C API symbol が存在する。
- `libmaleicacid_arib_caption_jni.so` が `libaribcaption.so` を明示依存する。
- `MaleicacidTvInput` に `libaribcaption.so` が同梱される。
- Rust JNI が decoder-only ではなく renderer API を呼ぶ。
- Kotlin overlay が文字列ではなく renderer 出力画像を合成する。
- 字幕無効化、track 解除、session release で表示が消える。
- renderer 不在・初期化失敗・decode 失敗・render 失敗を成功扱いにしない。
```

## 10. 参考情報

- libaribcaption-android は ARIB STD-B24 字幕 decoder / renderer であり、Android では FreeType が必要である。
- CMake では `ARIBCC_NO_RENDERER` が renderer 無効化 option である。
- CMake では Android 判定時に FreeType renderer を有効にする構造である。
- renderer C API は `aribcc_renderer_alloc`, `aribcc_renderer_initialize`, `aribcc_renderer_append_caption`, `aribcc_renderer_render`, `aribcc_render_result_cleanup`, `aribcc_renderer_flush` を提供する。
- renderer 出力画像は RGBA8888、座標、幅、高さ、stride、bitmap pointer、bitmap size を持つ。
