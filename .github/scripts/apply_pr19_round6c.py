from pathlib import Path

p = Path("tis/DESIGN_JA.md")
text = p.read_text()

old_soong = "Soong導入はAOSP `prebuilts/misc/common/androidx-media3`と同じ`pom2bp`生成の`android_library_import` + `static_libs`方式を使い、製品固有module名でTIS APKへstatic linkする。platform側`androidx.media3.*` moduleへのfallback、異version混在、runtime download、Gradle解決は行わない。"
new_soong = "Soong導入はAOSP `prebuilts/misc/common/androidx-media3`と同じ`pom2bp`生成の`android_library_import` + `static_libs`方式を使う。製品root module名は`maleicacid_media3_common_1_5_1`、`maleicacid_media3_exoplayer_1_5_1`、`maleicacid_media3_extractor_1_5_1`に固定し、POM dependency closureにも`maleicacid_` prefixを付けてplatform moduleと衝突させない。TIS APKは必要なroot moduleを`static_libs`で明示参照する。platform側`androidx.media3.*` moduleへのfallback、異version混在、runtime download、Gradle解決は行わない。"
if text.count(old_soong) != 1:
    raise SystemExit(f"soong old count={text.count(old_soong)}")
text = text.replace(old_soong, new_soong, 1)

old_attr = "Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。audio出力はMedia3が所有するが、Android 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`によるTV app attribution chainを失ってはならない。現行productではMedia3 1.5.1の`DefaultRenderersFactory.buildAudioSink(...)`をoverrideし、`DefaultAudioSink.Builder(sessionContext)`へ1.5.1公開`DefaultAudioSink.AudioTrackProvider`を`setAudioTrackProvider(...)`で注入する。providerはMedia3から渡された`AudioTrackConfig`、`AudioAttributes`、`audioSessionId`をそのまま`AudioTrack.Builder`へ写像し、API 34の`setContext(sessionContext)`を追加してbuildする。TISはAudioTrackへのwrite、playback head、clock、buffer schedulingを所有せず、AudioTrack生成だけをattribution付きfactory境界でMedia3へ返す。1.9系`AudioOutputProvider`、`AudioTrackAudioOutputProvider`、`setAudioTrackBuilderModifier(...)`には依存しない。通常経路で素の`serviceContext`へ後退せず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。session releaseまたはplayer置換後は旧`sessionContext`、旧player、旧AudioSinkを新generationへ再利用しない。"
new_attr = "Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。audio出力はMedia3が所有するが、Android 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`によるTV app attribution chainを失ってはならない。現行productではMedia3 1.5.1の`DefaultAudioTrackProvider`を継承したsession固有providerを使い、その1.5.1公開protected hook `customizeAudioTrackBuilder(AudioTrack.Builder)`だけをoverrideして`setContext(sessionContext)`を追加する。`DefaultRenderersFactory.buildAudioSink(...)`のoverrideでは`DefaultAudioSink.Builder(sessionContext).setAudioTrackProvider(sessionProvider).build()`を返す。sample rate、channel config、encoding、buffer size、audio attributes、audio session id、offload等のAudioTrack構成は`DefaultAudioTrackProvider`の標準実装に残し、TIS側へ複製しない。TISはAudioTrackへのwrite、playback head、clock、buffer schedulingを所有しない。Media3 1.9系で追加された別provider APIには依存しない。通常経路で素の`serviceContext`へ後退せず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。session releaseまたはplayer置換後は旧`sessionContext`、旧player、旧AudioSinkを新generationへ再利用しない。"
if text.count(old_attr) != 1:
    raise SystemExit(f"attr old count={text.count(old_attr)}")
text = text.replace(old_attr, new_attr, 1)

p.write_text(text)
