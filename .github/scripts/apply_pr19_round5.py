from pathlib import Path

p = Path("tis/DESIGN_JA.md")
s = p.read_text()


def rep(old: str, new: str, label: str) -> None:
    global s
    count = s.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1, got {count}")
    s = s.replace(old, new, 1)


rep(
    "`getLinearBlock()`がnull、block model configureまたはQueueRequestが利用不能、offset／lengthがblock範囲外、decoderが当該blockを受理しない場合は`BLOCK_MODEL_UNAVAILABLE`または入力不正の型付き診断へ落とす。成功を偽装せず、現generationのfilter、未queue event、decoder、MediaSync、startup queue、budget claimを解放して`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。",
    "`getLinearBlock()`がnull、offset／lengthがblock範囲外、Media3入力adapterへ安全に渡せない場合は`PLAYBACK_INPUT_UNAVAILABLE`または入力不正の型付き診断へ落とす。成功を偽装せず、現generationのfilter、未消費event、Media3 player／MediaSource／SampleStream adapter、startup queue、budget claimを解放して`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。",
    "cleanup",
)

old = """デコード後のA/V同期とSurface提示は、Android platformまたはAndroidXの標準renderer/player機構に委ねる。TIS自身は独自media clock、`AudioTimestamp`由来の独自同期、独自future render／late drop scheduler、固定delayを持たない。video側には設計上の役割として`VideoPresentationRenderer`境界を置く。この境界は特定ライブラリのクラス名ではなく、decoderからcurrent `sessionSurface`までのvideo scheduling／dropを最終的に所有し、dropしたframeとcurrent output Surfaceへrenderしたframeを区別できるrenderer契約を表す。最終rendererは、current playback generationかつcurrent Surface generationの非drop frameをcurrent `sessionSurface`へrenderした後にだけfirst-frame-rendered eventを返す。このeventは物理display/compositorのpresentation fenceを意味せず、TIFが要求する「content rendered onto its surface is ready for viewing」を判定するrenderer境界のcommitとする。

`MediaCodec.OnFrameRenderedListener`は、codecのoutput Surface自体がcurrent `sessionSurface`であり、その後段にvideo scheduling／dropを行う層が存在しない構成でのみfinal-renderの根拠にできる。codec outputが`MediaSync.createInputSurface()`で得たMediaSync入力Surfaceである構成では、`onFrameRendered()`はdecoderからMediaSync入力への到達を示す中間観測に限定する。`MediaSync.getTimestamp()`はcurrent playback positionの観測に過ぎず、MediaSync内部で当該video frameがrenderされたかdropされたかを区別できないため、first-frame availabilityの確定根拠または代替commitには使わない。公開`MediaSync` APIだけでMediaSync出力側のrender/dropを区別するfirst-frame eventを取得できない構成は、現行productの完成したvideo availability経路として採用しない。

適合する標準renderer構成は、最終output Surfaceへのfirst-frame-rendered eventとdrop eventを分離して公開し、A/V同期とframe release schedulingをその標準renderer/player側が所有することを必須とする。Media3を採用する場合は、`VideoSink.Listener.onFirstFrameRendered()`／`onFrameDropped()`相当を最終renderer eventとして使い、frame schedulingをTIS独自loopで再実装せずMedia3 renderer側に所有させる。AOSP Live TV系のように最終Surfaceへのdraw完了をrendererからsessionへ返す構成も同じ契約を満たし得る。特定renderer製品の採用自体は本設計では固定せず、上記境界を満たすAndroid標準経路であることを固定する。

`notifyVideoAvailable()`は、current playback generation/current Surface generationに結び付いた最終rendererのfirst-frame-rendered eventを受け、current `sessionSurface`が有効、視聴制限でblockされておらず、同generationのrenderer／Surface failureがない場合だけ一度呼ぶ。frame-available-before-render、decoder output生成、MediaSync入力到達、media clockのcandidate PTS到達、drop eventだけでは通知しない。旧generation／旧Surfaceのcallbackは無視する。固定delay、独自clock、独自frame scheduler、hidden API、Surface/compositor pixel probeは追加しない。audio bufferの所有権と寿命は選択した標準renderer/playerの公開契約に従う。"""
new = """デコード、A/V同期、video frame scheduling／drop、AudioTrack、Surface提示は現行productではAndroidX Media3 ExoPlayerへ一括して委ねる。TIS自身はMediaCodec、MediaSync、AudioTrack、独自media clock、独自future/late schedulerをplayback ownerとして持たない。`ExoPlayer.Builder(sessionContext)`でcurrent playback generation専用playerを生成し、TISはTuner AV filterから受けた圧縮sampleをMedia3 `MediaSource`／`SampleStream` adapterとして供給する。video outputは`player.setVideoSurface(currentSessionSurface)`でcurrent `sessionSurface`へ直接設定し、audio output、decoder選択、A/V clock、frame release scheduling、late dropはMedia3 renderer群へ所有させる。

Tuner `MediaEvent.getLinearBlock()`はTIS input adapterまでの所有物とする。Media3 `SampleStream.readData()`の公開契約は`DecoderInputBuffer.data`の`ByteBuffer`へsample dataを供給する形なので、現行productではadapterが`LinearBlock`の有効rangeをread-only mapし、そのrangeをcurrent `DecoderInputBuffer`へ1回copyしてPTSとflagsを設定する。copy完了後に該当`MediaEvent`／`LinearBlock`とTIS budget claimを解放する。Media3へ渡した後のcompressed input buffer、decoder output、audio buffer、render queueの寿命はMedia3が所有する。従来のTIS-owned `MediaCodec.CONFIGURE_FLAG_USE_BLOCK_MODEL`／`QueueRequest.setLinearBlock()` zero-copy経路は、このownership graphと公開Media3入力契約を同時には満たせないため現行productの正式playback経路から外す。header解析のためのmapとMedia3 adapterへの1回copy以外にES全体の多重copyやByteArray中継を追加しない。

first-frame availabilityのcommitはMedia3 `Player.Listener.onRenderedFirstFrame()`を使う。このcallbackはsurface設定、renderer reset、stream変更後にframeが初めてrenderされた時点のイベントなので、current playback generationとcurrent Surface generationへlistener tokenを結び付ける。`notifyVideoAvailable()`はこのcurrent tokenの`onRenderedFirstFrame()`を受け、current `sessionSurface`が有効、視聴制限でblockされておらず、同generationのplayer／video renderer errorがない場合だけ一度呼ぶ。Media3内部のframe-available、decoder output、clock進行、drop、旧generation／旧Surface callbackをfinal commitへ昇格させない。物理display/compositor fenceは要求せず、固定delay、独自clock、独自frame scheduler、hidden API、pixel probeも追加しない。"""
rep(old, new, "main renderer ownership")

rep(
    "TIS のライブplaybackは、Tuner AV filterの平文`MediaEvent.LinearBlock`をMediaCodec block modelへcopyなしで投入する入力契約を維持する。decoder以後のA/V同期、video scheduling／drop、current `sessionSurface`への提示は、本書「再生経路」の`VideoPresentationRenderer`境界を満たすAndroid platform／AndroidX標準renderer/playerへ委ねる。TISはその外側に独自clockまたは独自frame schedulerを置かない。codec outputを`MediaSync.createInputSurface()`へ入れ、MediaSync前段の`OnFrameRendered`と`getTimestamp()`だけで最終availabilityを確定する構成は採用しない。",
    "TIS のライブplaybackは、Tuner AV filterの`MediaEvent.LinearBlock`をTISのMedia3 `SampleStream` adapterで受け、必要rangeを`DecoderInputBuffer`へ1回copyしてMedia3 ExoPlayerへ供給する経路に固定する。ExoPlayerがdecoder、audio sink、A/V clock、video scheduling／drop、current `sessionSurface`への提示を所有し、TISはその外側にMediaCodec／MediaSyncや独自clock／frame schedulerを置かない。",
    "live playback",
)
rep(
    "`tunneled`／platform passthrough playback pathは現行productの設計候補から外し、実装しない。`notifyVideoAvailable()`は、video scheduling／dropを最終的に所有するrendererがcurrent generation/current `sessionSurface`へ非dropのfirst frameをrenderしたeventを唯一のvideo成功commitとして扱い、current Surface有効、generation一致、視聴制限、renderer／Surface errorの各gateを満たした場合だけ一度通知する。frame available、decoder output、MediaSync入力到達、`MediaSync.getTimestamp()`のmedia position到達、drop eventをfinal commitへ昇格させない。",
    "`tunneled`／platform passthrough playback pathは現行productの設計候補から外し、実装しない。`notifyVideoAvailable()`は、current player/current Surface tokenに対するMedia3 `Player.Listener.onRenderedFirstFrame()`だけをvideo成功commitとして扱い、current Surface有効、generation一致、視聴制限、player／video renderer errorの各gateを満たした場合だけ一度通知する。frame available、decoder output、clock進行、drop eventをfinal commitへ昇格させない。",
    "live notify",
)

start = "A/V同期方式は現行productでAndroid platformまたはAndroidXの標準renderer/player機構に固定し、TIS独自のmedia clock、frame release clock、future/late判定を実装しない。"
end = "\n\nTvProvider公開モードは"
if s.count(start) != 1:
    raise SystemExit(f"ownership section start: {s.count(start)}")
i = s.index(start)
j = s.index(end, i)
replacement = """A/V同期方式とownership graphは現行productでMedia3 ExoPlayerに固定する。TISはTuner filter／Media3 input adapter／session lifecycleだけを所有し、ExoPlayerがdecoder、audio sink、playback clock、video renderer、frame scheduling／dropを所有する。`PlaybackPipeline`のserial executorはcurrent player generation、Surface generation、Tuner filter、input adapter、pending `MediaEvent`／`LinearBlock`、budget claim、player listener tokenを単一管理し、player callback／Tuner callback／parental callbackはstateを直接変更せず同executorへ直列化する。

input adapterはMedia3 `MediaSource`／`SampleStream`として実装し、Tuner sampleのPTS、codec format、EOSをMedia3へ公開する。`SampleStream.readData()`がsampleを要求した時だけpending `LinearBlock`の対象rangeをread-only mapして`DecoderInputBuffer.data`へ1回copyし、`timeUs`と必要flagsを設定する。copyが完了したsampleはTuner側ownershipとbudget claimを即時返却する。Media3が受理した後のbuffer lifetime、decoder input/output、audio queue、video frame queueはExoPlayer内部ownershipとし、TISはcodec output IDやAudioTrack bufferを保持しない。pending queue満杯時だけ既存budget規則で入力sampleをdropし、それ以外を無通知破棄しない。

video outputは`player.setVideoSurface(sessionSurface)`でcurrent Surfaceへ設定する。Surface設定または変更ごとにSurface generationを進め、`Player.Listener.onRenderedFirstFrame()`をcurrent player generation/current Surface generationへ関連付ける。Media3 Player契約上、このcallbackはsurface設定、renderer reset、stream変更後のfirst rendered frameを通知するため、これをTIF availability commitとして使用する。TISはMedia3内部のframe release時刻計算、late判定、drop判定を再実装しない。

audio outputは`ExoPlayer.Builder(sessionContext)`から生成したplayerの標準audio renderer／AudioSinkへ所有させる。session attributionはplayer生成Contextとして`sessionContext`を渡すことで同generationへ閉じる。TISが別途AudioTrackを生成してplayer外から同期させる経路は持たない。video-onlyではaudio trackを選択しない／audio rendererを無効化し、audio-onlyではvideo Surfaceを設定せず`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`契約を維持する。

retune、playback generation変更、stop、非wrap PTS discontinuity、decoder fatal、player fatalではcurrent player、MediaSource、SampleStream adapter、pending sampleをreleaseして新player generationを作る。Surface変更だけではplayer全体を必須再生成せず、旧Surfaceをclearして新Surfaceを設定しSurface generationを進める。旧player generationまたは旧Surface generationのlistener callbackは状態更新に使わない。通常33-bit PTS wrapはadapter内でunwrapし、wrapだけではgenerationを変更しない。

最低試験契約は、Tuner sample→Media3 SampleStream adapter、LinearBlock range検証、Media3入力への1回copy、PTS欠落sample単体dropとgeneration継続、通常PTS wrap、`onRenderedFirstFrame()`前はvideo availableにしないこと、current player/current Surface token以外のfirst-frame callbackを無視すること、drop／clock進行だけでは通知しないこと、Surface／parental／player-error gate成立後に一回だけavailability通知すること、retune／fatal後の旧player callback非採用、Surface切替後の旧Surface callback非採用、audio/video-only、player release時のpending Tuner ownership回収を含む。試験のqueue数値上限は選択した`ProductProfile`と一致させる。"""
s = s[:i] + replacement + s[j:]

rep(
    "`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。filter、block model decoder、MediaSync、MediaSync input Surface、AudioTrack、generation、surface、未返却audio buffer id、トークンの変更を呼び出し元スレッドで直接行わない。release後のqueued taskはreleased flagとgenerationで破棄する。",
    "`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。Tuner filter、Media3 MediaSource／SampleStream adapter、ExoPlayer、player generation、Surface generation、pending Tuner sample、budget claim、listener tokenの変更を呼び出し元スレッドで直接行わない。release後のqueued taskはreleased flagとgenerationで破棄する。",
    "serialization",
)
rep(
    "- decoder／MediaSync入力の逆圧は無通知破棄ではない。sampleまたは未返却audio outputは上限付きpending queueとbudget claimに保持し、後続callback／drainで再試行する。sampleを破棄するのは上限付きqueueが満杯の場合だけとし、破棄counterを加算する。",
    "- Tuner→Media3 input adapterの逆圧は無通知破棄ではない。未読`MediaEvent`／`LinearBlock`は上限付きpending queueとbudget claimに保持し、Media3 `SampleStream.readData()`で消費する。sampleを破棄するのは上限付きqueueが満杯の場合だけとし、破棄counterを加算する。",
    "backpressure",
)
rep(
    "生成したAudioTrackは同generationのMediaSyncへ設定し、session releaseまたは置換後は旧`sessionContext`と旧AudioTrackを新しいMediaSync generationへ再利用しない。",
    "現行playbackではTISがAudioTrackを直接生成せず、`ExoPlayer.Builder(sessionContext)`で同generationのMedia3 playerを生成し、その標準audio renderer／AudioSinkにaudio出力を所有させる。session releaseまたはplayer置換後は旧`sessionContext`と旧playerを新generationへ再利用しない。",
    "attribution",
)
rep("MediaFormat、block model decoder起動、MediaSync first-frame gate、unsupported 診断情報を固定する。", "Media3 Format写像、decoder capability確認、`onRenderedFirstFrame()` gate、unsupported 診断情報を固定する。", "mpeg2 codec")
rep("MediaFormat / block model decoder / MediaSync first-frame gate まで必須。", "Media3 Format写像 / decoder capability確認 / `onRenderedFirstFrame()` gate まで必須。", "hevc codec")
rep("block model decoder / MediaSync / AudioTrack / メタデータ / unsupported 診断情報 まで必須。", "Media3 Format写像 / decoder capability確認 / audio renderer／AudioSink / メタデータ / unsupported 診断情報 まで必須。", "als codec")
rep(
    "- `notifyVideoAvailable()` は、video scheduling／dropを最終的に所有する`VideoPresentationRenderer`がcurrent playback generation/current Surface generationの非drop first frameをcurrent `sessionSurface`へrenderしたeventを受けた後だけ一度呼ぶ。`MediaCodec.OnFrameRenderedListener`がMediaSync入力Surface到達を示すだけの構成、`MediaSync.getTimestamp()`のmedia position、frame-available event、drop eventはavailability確定根拠にしない。物理display/compositor fenceは要求しないが、最終rendererより前段のcallbackで代用もしない。固定delay、独自clock、独自frame scheduler、hidden API、pixel probeは使わない。",
    "- `notifyVideoAvailable()` はcurrent Media3 player/current Surface generationの`Player.Listener.onRenderedFirstFrame()`を受けた後だけ一度呼ぶ。clock進行、decoder output、drop、旧player／旧Surface callbackはavailability確定根拠にしない。物理display/compositor fenceは要求しないが、rendererより前段のcallbackで代用もしない。固定delay、独自clock、独自frame scheduler、hidden API、pixel probeは使わない。",
    "fixed notify bullet",
)

p.write_text(s)
