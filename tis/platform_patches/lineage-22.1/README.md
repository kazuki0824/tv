# LineageOS 22.1 MediaSync first-output integration

TIS の `notifyVideoAvailable()` 契約で使用する `MediaSync` の platform-private first-output observation を LineageOS 22.1 platform checkout に追加する。

このディレクトリの patch は `tv` repo 内に Android framework 実装を複製するためのものではない。実装 owner は LineageOS platform の `frameworks/base` / `frameworks/av` のままとし、この repo は再現可能な統合差分だけを保持する。

## 適用

Android build root から次を実行する。

```bash
git -C frameworks/av apply --check \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tis/platform_patches/lineage-22.1/frameworks_av_mediasync_first_output.patch"
git -C frameworks/base apply --check \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch"

git -C frameworks/av apply \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tis/platform_patches/lineage-22.1/frameworks_av_mediasync_first_output.patch"
git -C frameworks/base apply \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch"
```

対象 baseline は LineageOS 22.1 の次のファイルである。

```text
frameworks/av:
  media/libstagefright/include/media/stagefright/MediaSync.h
  media/libstagefright/MediaSync.cpp

frameworks/base:
  media/java/android/media/MediaSync.java
  media/jni/android_media_MediaSync.h
  media/jni/android_media_MediaSync.cpp
```

## 契約

- public SDK、`@SystemApi`、`@TestApi`、Tuner AIDL/VINTFは変更しない。
- `android.media.MediaSync` に `@hide OnFirstVideoFrameQueuedToOutputListener` と `@hide setOnFirstVideoFrameQueuedToOutputListener(long armSequence, listener, handler)` を追加する。
- `armSequence` は TIS 固有意味を platform に持ち込まない opaque 値である。
- native MediaSync が late-drop を通過し、current output への `attachBuffer()` と `queueBuffer()` が成功した場合だけ one-shot arm を消費する。
- late-drop、attach失敗、queue失敗、output abandonmentでは成功eventを生成しない。
- queue成功時点の `armSequence` を非同期eventへ固定し、後続re-armで書き換えない。
- `MediaSync` の native mutex を保持したまま JNI / Java listener を呼ばない。queue成功時は MediaSync looper へeventをpostし、mutex外で JNI bridge を呼ぶ。
- Java側は指定Handlerへ非同期配送する。
- TISは `MediaSync instance + playback generation + armSequence` を照合して stale event を拒否する。

## 確認

patch適用後は少なくとも次を実施する。

```bash
m framework-minus-apex
m libstagefright
m MaleicacidTvInput
```

対象treeのmodule分割により上記module名が利用できない場合は、`frameworks/base` の media JNI / framework Java と `frameworks/av` の MediaSync を実際に再コンパイルする同等のSoong targetを使用する。

加えて、TIS host CIは型付き呼出しを確認するだけであり、platform patchのnative意味論を代替しない。target buildではJava/JNI/nativeの型接続を確認し、実機では late-drop がavailabilityを成立させないこと、output queue成功が一回だけ通知されること、re-arm後に遅延した旧sequence eventがTISで棄却されることを確認する。
