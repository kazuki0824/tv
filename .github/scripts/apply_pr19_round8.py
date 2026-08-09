from pathlib import Path
import re

p = Path("tis/DESIGN_JA.md")
text = p.read_text()


def replace_section(src: str, start: str, end: str, replacement: str) -> str:
    pattern = re.compile(re.escape(start) + r".*?(?=" + re.escape(end) + r")", re.S)
    out, n = pattern.subn(replacement.rstrip() + "\n\n", src, count=1)
    if n != 1:
        raise SystemExit(f"section replace failed: {start!r} -> {end!r}: {n}")
    return out

# Replace the entire playback ownership section.  The public MediaSync API remains unchanged;
# the product-only observation hook is deliberately a separate @hide listener.
playback = r'''## 再生経路

### MediaSync Framework-private first-output commit 境界

現行productの平文 non-tunneled playback は、AOSP Tuner framework が明示する `Tuner MediaEvent.LinearBlock -> MediaCodec -> AudioTrack` の流れを基礎とし、A/V同期とvideo frame scheduling/dropはplatform `MediaSync`へ委ねる。AndroidX Media3を現行productのplayback dependencyにはしない。product-local Media3 AAR、Media3 fork/backport、`MediaSource` / `SampleStream` adapter、およびそれらに伴うES copyは導入しない。

stock Android 14 / LineageOS 21 の `MediaSync` は、video frameをlate-dropする分岐と、render対象frameをcurrent outputへ`queueBuffer()`する分岐をnative側で区別しているが、その最終output queue成功をJava clientへ返す公開callbackを持たない。この不足だけを閉じるため、対象LineageOS platformに **Framework-privateなfirst-output callbackを最小追加**する。既存のpublic `MediaSync.Callback`へmethodを追加してはならず、`android.media.MediaSync`に別の `@hide OnFirstVideoFrameQueuedListener` と `@hide setOnFirstVideoFrameQueuedListener(listener, handler)` 相当だけを追加する。public SDK、`@SystemApi`、`@TestApi`、Tuner AIDL/VINTFには追加・変更しない。

native `MediaSync` は、current instanceについて最初のvideo bufferが `onDrainVideo_l()` のlate-drop分岐を通過し、current `mOutput`へのattachと`queueBuffer()`がともに成功した後だけ、one-shotの「first video frame queued to output」eventを生成する。late-drop、attach失敗、queue失敗、output abandonment、inputへ返したbufferではeventを生成しない。名称・契約上も「物理displayへpresent済み」やcompositor fence完了とは主張しない。eventはnative内部mutexを保持したままJavaへreentrant callせず、JNI/Java handlerへ非同期配送する。`release()`済みinstanceの未配送eventは状態更新に使わない。

このcallbackはAndroid標準公開APIではなく、同一製品buildでFrameworkと同時更新されるplatform-private contractである。Android API namespaceへpublic/System/Test APIを追加せず`@hide`のまま維持する。consumerであるTIS APKは `/system_ext` のplatform-coupled componentとして配置し、型付きprivate APIとして直接compileする。`/product`からhidden APIを呼ぶ構成、reflection、hidden API allowlist抜け道、callback不存在時のtimestamp推測fallbackは使わない。Framework patchがないbuildはこのproduct playback contractを満たさないものとしてbuild/integration時に失敗させる。

### Tuner LinearBlock / MediaCodec block-model 入力

平文video/audio AV filterから受けた `MediaEvent.getLinearBlock()` は、`MediaCodec.CONFIGURE_FLAG_USE_BLOCK_MODEL` で構成した対応decoderへ直接queueする。TISは `offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= LinearBlock capacity` をqueue前に検証する。codec header解析に必要な最小rangeだけread-only `map()`してよいが、ES本体を`ByteArray`、Media3 buffer、別LinearBlockへcopyしない。

`MediaEvent.isPtsPresent()` がtrueのsampleでは、raw PTSを90 kHz / 33-bit moduloとして扱う。`PlaybackPipeline`はplayback generationごとに`PtsNormalizer`を1個だけ所有し、連続raw PTS差をmodulo `2^33`の最短signed差としてunwrapし、`presentationTimeUs = floor(pts90k * 1000000 / 90000)`をchecked integer arithmeticで算出する。codec input slotを得たら `MediaCodec.getQueueRequest(index).setLinearBlock(linearBlock, offset, dataLength).setPresentationTimeUs(presentationTimeUs)` に必要flagsを設定して`queue()`し、成功後はTIS側の`LinearBlock`参照を`recycle()`してpending claimを返す。framework/codecが処理中の実buffer寿命は`recycle()`後もframeworkへ委ねる。

`isPtsPresent()==false`はAOSP Tuner API上で表現可能な入力状態として扱い、0、直前sample、PCR、wallclock等から時刻を捏造しない。当該sampleだけをcodecへqueueせず解放してtrack別`MISSING_PTS_SAMPLE`へ計上し、単発欠落だけでplayback generationをteardownしたり`notifyVideoUnavailable()`したりしない。first frame前でも既存`decoderStartupDeadlineMs`を延長・resetしない。generation failureはdecoder/MediaSync/Surface fatal、lock喪失、startup/backpressure deadline等の独立条件だけで判定する。

`PtsNormalizer`はretune、playback generation変更、filter flush、decoder/MediaSync再生成、非wrap discontinuityで破棄する。通常33-bit wrapだけは同generationでunwrapし、PCR→wallclock変換や別media clockへ拡張しない。secure `MediaEvent`は現行平文product対象外とし、mappable clear blockへの暗黙変換を行わない。

### MediaCodec / MediaSync / AudioTrack ownership

video decoderのoutput Surfaceはcurrent `MediaSync.createInputSurface()`に固定する。decoder output callbackで非decode-only frameをそのmedia PTSに対応するSurface timestamp付きでreleaseし、その後のdue-time計算、vsync scheduling、late/drop判定、current `sessionSurface`へのqueueはMediaSyncだけが所有する。TISは`MediaCodec.OnFrameRenderedListener`や`MediaSync.getTimestamp()`をvideo availabilityの代替commitとして使わず、video scheduling/dropを再実装しない。

audioは対応decoderでPCMへdecodeし、block-model `OutputFrame`のlinear outputがmappableであるproduct profileだけを現行経路の対応対象とする。decoded rangeをmapした`ByteBuffer`を`MediaSync.queueAudio(buffer, bufferId, presentationTimeUs)`へ渡し、`MediaSync.Callback.onAudioBufferConsumed()`で返却されるまで対応codec outputを保持し、そのcallback後にreleaseする。mappable PCM outputを成立させられないdecoder/profileを無言copy fallbackで成功扱いしない。

`AudioTrack`はTISがsessionごとに直接生成し、Android 14公開 `AudioTrack.Builder.setContext(sessionContext)` を必ず設定してTV app attribution chainを保持する。sample rate、channel mask、encoding、buffer size、AudioAttributes等は選択codec/output formatと製品profileから型付きで設定し、生成したAudioTrackを同generationのMediaSyncへ`setAudioTrack()`する。TISはAudioTrackをmedia clockとして独自解釈せず、A/Vのcanonical media clockはMediaSyncだけが所有する。video-onlyではAudioTrack/audio decoderを開始せず、audio-onlyではvideo decoder/Surfaceを開始せず`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`を維持する。

### video availability / generation

`notifyVideoAvailable()`の唯一のvideo成功commitは、**current playback generationのcurrent MediaSync instanceから受けたfirst-output callback**とする。callback受信時にcurrent `sessionSurface`が有効、視聴制限でblockされていない、同generationのMediaSync/Surface/video decoder fatalがないことを確認し、そのgenerationについて一度だけ通知する。decoder output、`MediaCodec.OnFrameRenderedListener`、`MediaSync.getTimestamp()`のclock進行、late-drop、旧MediaSync instanceのcallbackをavailabilityへ昇格させない。

このcommitはphysical compositor present fenceを要求しない一方、stock MediaSync案の弱点だった「scheduler/drop ownerより前段しか観測できない」問題を残さない。native MediaSync自身がlate-drop後にrender対象を選択し、current final outputへの`queueBuffer()`成功を確認した後のeventだけをTISへ渡すため、TIFのsurface content ready-for-viewing判定に使う最小のplatform観測点とする。

`MediaSync.setSurface()`は初期化後のSurface差替えを通常経路にしない。retune、stop、非wrap discontinuity、decoder/MediaSync fatalに加え、**Surface変更もpresentation generation境界**としてcurrent MediaSyncをreleaseし、新しい`sessionSurface`を設定したMediaSyncを生成する。video decoderのoutputは新MediaSync input Surfaceへ再bindまたはdecoder再生成し、どちらを使う場合も旧instance callbackをgeneration tokenで破棄する。これによりnative Surface-generation payloadを追加せずstale callbackを閉じる。通常33-bit PTS wrapだけではgenerationを変更しない。

最低Framework試験は、late-dropではcallbackなし、attach/queue失敗ではcallbackなし、最初の成功`queueBuffer()`後だけone-shot callback、2 frame目以後は再通知なし、release後のstale event非採用、video-onlyでも最初の成功queueで通知、public SDK/API signature不変を含む。TIS試験はcallback前に`notifyVideoAvailable()`しないこと、旧generation callbackを無視すること、Surface変更でMediaSync再生成すること、parental/error gate、audio/video-onlyを含む。Tuner HAL AIDL/VTS contractは変更しない。
'''
text = replace_section(text, "## 再生経路\n", "## EIT と TvProvider\n", playback)

subtitle = r'''### 字幕PTS scheduling / clear ownership

字幕のmedia clockはvideo/audioと同じ **current MediaSyncのcanonical clockだけ** とする。TIS、Rust JNI、`CaptionOverlayView`はPCR/wallclockから別media clockを作らず、固定delayや周期的な`getTimestamp()` polling loopも持たない。ARIB字幕PESから得たPTSはvideo/audioと同じ33-bit 90 kHz unwrap規則でcurrent playback generationの`timeUs`へ変換し、libaribcaption decoder/rendererへ渡す。rendererが返したRGBA8888画像とrender regionはin-processでoverlay所有のBitmap等へ直接受け渡しし、PNG/Parcel等のserialization round-tripを挟まない。

字幕display/clearは、MediaSyncの`getTimestamp()`が返すmedia time / anchor time / playback rateを唯一の時間基準とする **event-driven one-shot subtitle scheduler** が担当する。新caption、finite-durationのclear、明示clearのうち次の1境界だけをarmし、予定時刻到達時にcurrent MediaSync timestampを再読して境界到達を確認する。まだearlyなら同じ境界へre-armし、dueならdisplay/clearして次境界だけをarmする。周期polling、独立free-running clock、PCR→wallclock clock、video frame release/drop判定を実装しない。playback rate変更、flush、retune、Surface/MediaSync generation変更、track disable、session releaseではpending subtitle eventをcancel/re-armする。

libaribcaptionが有限`wait_duration`を返すcaptionは`PTS + duration`で明示clearを予約する。`DURATION_INDEFINITE`は有限値へ推測変換せず、次caption、ARIB/libaribcaptionの明示clear、字幕track無効化、generation終了までcurrent imageを保持する。次captionはそのPTS境界で旧imageを直接replaceする。既に表示済みcaptionのdurationを後から別clockで補正しない。`onSelectTrack(TYPE_SUBTITLE, null)`、retune、Surface/session release、playback generation変更は即時にscheduler stateとoverlayをclearし、旧generationのlibaribcaption結果/eventはgeneration tokenで破棄する。

このschedulerはA/V clockやvideo schedulerを複製するものではなく、MediaSyncが所有するcanonical playback positionに字幕presentation eventを従属させるUI dispatch層である。既存future_workの「libaribcaption rendererのRGBA8888をKotlin overlayへ表示する」という完了条件をそのまま維持し、Media3 bitmap Cue/PNG経路は要求しない。
'''
text = replace_section(text, "### 字幕PTS scheduling / clear ownership\n", "## ライブ playback 実装方式\n", subtitle)

live = r'''## ライブ playback 実装方式

TISのライブplaybackは、`Tuner AV filter -> MediaEvent.LinearBlock -> MediaCodec block model -> MediaSync -> current sessionSurface / AudioTrack`に固定する。TISはTuner filter、decoder feed、AudioTrack生成、MediaSync instance/generation lifecycle、字幕event dispatchを所有する。MediaSyncは唯一のA/V media clockとvideo frame scheduling/drop ownerであり、TISは独自A/V clock、独自video frame scheduler、fixed-delay availability判定を持たない。

video decoderはcurrent MediaSync input Surfaceへ出力し、current MediaSyncのFramework-private first-output callbackだけを`notifyVideoAvailable()`成功commitにする。Surface変更ではMediaSyncを再生成してstale callbackをinstance/generationで排除する。audio decoderのPCMは`MediaSync.queueAudio()`へ渡し、TIS生成AudioTrackはMediaSyncへ接続する。

`tunneled`／platform passthrough playback pathは現行productの設計候補から外し、実装しない。stock MediaSyncだけでfirst-output観測を推測するfallback、`OnFrameRendered + getTimestamp()` gate、hidden callbackのreflection探索、pixel probe、compositor fence待ちを通常経路に置かない。

setup scan の channel registration は global discovery complete を必須条件にしない。ただし partial snapshot を無条件に channel insert に使ってはならない。TvProvider のサービス単位の登録可否は本書の「サービス登録・publishability利用境界」を唯一の正本とし、この節で video ES 必須などの追加 gate を重複定義しない。したがって `service_type=0x01` は同節の audio-video / video-only 条件、`service_type=0x02` は対応 audio ES を持つ audio-only 条件に従い、`0x02` の登録に video ES を要求しない。登録可能未満の partial snapshot は 診断情報 / ライブ更新 / debugにのみ使い、channel insert しない。scrambled サービスは channel 登録してよいが、CAS 仮実装 のまま 平文ライブ視聴成功 対応宣言 してはならない。
'''
text = replace_section(text, "## ライブ playback 実装方式\n", "## codec header / A-V sync / publish mode の固定\n", live)

codec = r'''## codec header / A-V sync / publish mode の固定

ライブ playback の codec 構成は、現行 product では video は MPEG-2 video と H.264/AVC、audio は AAC と MPEG audio を対象 codec とする。現行 product が対象とする transport profile で追加 codec を扱う場合は、`開発規則.md` の ARIB 本文選定規則に従う条項根拠と、MediaFormat、MediaCodec block-model decoder、AudioTrack、MediaSync first-output gate、unsupported 診断情報の契約を設計正本へ固定してから扱う。STD-B79 / STD-B80 の高度地上方式が現行 product scope 外である間、それらの方式だけに追加された codec を現行 playback capabilityへ入れない。

現行製品が登録対象とするARIB `service_type`は、`0x01`のdigital television serviceと`0x02`のdigital radio sound serviceに固定する。`0x01`は`TvContract.Channels.SERVICE_TYPE_AUDIO_VIDEO`、`0x02`は`TvContract.Channels.SERVICE_TYPE_AUDIO`へ写像する。その他のservice typeは壊れたサービスへ丸めず、`UNSUPPORTED_SERVICE_TYPE`を記録して現行製品スコープ外としてchannel登録しない。対応集合を追加する場合は、ARIB上の意味、TvProvider写像、PMT成立条件、再生経路を同じ変更で追加する。

`service_type=0x02`は本来的なaudio-only serviceである。少なくとも1本の現行対応audio ESと物理選局情報、`ServiceKey`、inputId、表示名が揃えば、video ESを要求せず`SERVICE_TYPE_AUDIO`として登録し、audio filter・decoder・AudioTrackだけを開始する。視聴sessionでは映像filterを開かず、サービス分類確定後に`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)`を通知し、audio再生の成否と映像なし通知を分離する。audio codec非対応またはaudio ES欠落は`AUDIO_ONLY`の正常理由ではなく、`UNSUPPORTED_AUDIO_CODEC`または`SERVICE_TYPE_PMT_MISMATCH`として再生不能にする。

`service_type=0x01`はaudio-video serviceであり、現行対応video ESがない場合にaudio-onlyへ再分類しない。弱信号またはlock喪失は`VIDEO_UNAVAILABLE_REASON_WEAK_SIGNAL`、有効なserviceでdecoder起動またはqueue補充を一時待機する場合だけ`VIDEO_UNAVAILABLE_REASON_BUFFERING`、video codec非対応またはPMT構成不整合は`VIDEO_UNAVAILABLE_REASON_UNKNOWN`と型付き診断`UNSUPPORTED_VIDEO_CODEC`／`SERVICE_TYPE_PMT_MISMATCH`へ分離する。HEVCなど未対応codecのmetadataはprovider-dataへ保存してよいが、再生可能表明には使わない。

現行対応 video ES が存在し、audio ES が存在しない、または audio codec だけが現行未対応の場合は、video-only サービスとして視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。STD-B32 4.0以降の改定概要で高度地上デジタルテレビジョン放送向けに追加された MPEG-H 3D Audio / AC-4 は、STD-B79 / STD-B80 の高度地上方式が現行product scope外であるため現行codec固定表へ追加しない。AC-3 / Enhanced AC-3 も現行対象transportに対する条項根拠を確認せず推測で追加しない。

PMTからcodec family、audio/video種別、PIDを確定した後、AV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。snapshotは製品profileで事前検証した有限値として、`singleEventLimitBytes`、`startupQueueBudgetBytes`、`startupQueueMaxSamples`、`startupQueueMaxDurationUs`、`pendingQueueBudgetBytes`、`pendingQueueMaxSamples`、`pendingQueueMaxDurationUs`、`decoderStartupDeadlineMs`、`steadyBackpressureDeadlineMs`を持つ。codec headerをまだ受信していないこと、decoderが未構成であること、codec input slot待ちであることを理由に値を動的導出しない。全codec共通の固定値へ丸めず、対象codec、decoder/device組合せ、最大access unit、header収集量、reorder depth、allocator上限、実機最悪値からofflineで検証する。正の有限値と必要領域を開始前に予約できないprofileはAV filterを開始しない。

startup queueと台帳claimを確保した後にAV filterを開始し、上限内の`MediaEvent`からMPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio headerを収集して`MediaFormat`へ写像する。header解析に必要な最小範囲だけ`LinearBlock.map()`でread-only参照してよいが、ES本体を別bufferへ複製してはならない。必要なformat情報が成立したらvideo/audio decoderを`CONFIGURE_FLAG_USE_BLOCK_MODEL`で構成し、video decoder outputをcurrent MediaSync input Surfaceへ接続する。codec input slot要求に応じてstartup/pending `LinearBlock`の有効rangeを`QueueRequest.setLinearBlock()`へ直接設定し、PTS/flagsを付けてqueueする。decoder capability不足、audio block-model output非mappable、decoder初期化失敗は型付き診断へ落とし、filter、pending sample、claim、decoder、MediaSync、AudioTrackを回収して既存unavailable規則へ進む。

MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= LinearBlock capacity`を満たす場合だけstartup/pending queueへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。

必要なqueue領域とclaim台帳はplayback generation開始時に原子的に予約する。各eventはrange検証後、codec input slot待ちqueueへenqueueする前に`dataLength`をsnapshot台帳へclaimし、いずれかのevent、byte、sample、duration上限を超える場合は原因別に`SAMPLE_TOO_LARGE`または`PENDING_QUEUE_FULL`を記録してHAL handleを解放する。claim済みbyte、sample、durationはcodecへの`QueueRequest.queue()`成功後のTIS参照`recycle()`、破棄、generation変更、stop、releaseで正確に返す。HAL側underlying blockのframework/codec処理寿命をTIS claimへ二重計上しない。

first frame前はcodec-specificな`decoderStartupDeadlineMs`を用い、必要header、decoder input/output、MediaSync startupを待つ間の一時queue増加を通常backpressure失敗へ写像しない。startup deadlineまでにdecoder入力可能状態またはMediaSync first-output callbackへ到達できず、queueのbyteまたはduration上限も解消しない場合だけplaybackを停止して`notifyVideoUnavailable()`へ進む。first frame後は別の`steadyBackpressureDeadlineMs`を用い、単発超過は当該sampleを解放して継続し、期限中にcodec dequeue/queue進行がなくqueue上限が継続する場合だけunavailableへ遷移する。audioだけの超過はvideo-only継続可否を既存規則で判定し、無条件にvideo unavailableへ写像しない。

A/V同期方式はcurrent MediaSync instanceへ固定する。TISはTuner filter、MediaCodec feed/output lifetime、AudioTrack生成、MediaSync instance lifecycleだけを所有し、MediaSyncがcanonical playback clockとvideo scheduling/dropを所有する。`PlaybackPipeline`のserial executorはcurrent playback generation、Tuner filter、video/audio decoder、MediaSync、AudioTrack、pending `MediaEvent`/`LinearBlock`、budget claim、first-output listener token、subtitle boundary queueを単一管理し、MediaSync/MediaCodec/Tuner/parental callbackはstateを直接変更せず同executorへ直列化する。

video outputはcurrent MediaSyncにcurrent`sessionSurface`を設定し、decoderを`createInputSurface()`へ出力する。MediaSync nativeのlate/drop分岐を通過しfinal outputへの`queueBuffer()`が成功した最初のframeだけがFramework-private callbackを発生させる。TISはこのcallbackをcurrent MediaSync instance/generationへ照合してTIF availability commitにする。

audio outputは`AudioTrack.Builder.setContext(sessionContext)`でTISが生成し、MediaSyncへ設定する。audio decoder PCM outputは`MediaSync.queueAudio()`で供給し、`onAudioBufferConsumed()`までcodec output lifetimeを保持する。video-onlyではaudio pathを作らず、audio-onlyではvideo Surface/decoderを作らない。

retune、playback generation変更、stop、非wrap PTS discontinuity、decoder/MediaSync fatal、**Surface変更**ではcurrent MediaSyncをreleaseして新instanceを作る。必要に応じvideo decoderも新MediaSync input Surfaceへ再bind/再生成する。旧instance listener eventは状態更新に使わない。通常33-bit PTS wrapだけではgenerationを変更しない。

最低試験契約は、Tuner LinearBlock range検証、MediaCodec block-model direct queue、queue後recycle、PTS欠落sample単体dropとgeneration継続、通常PTS wrap、late-dropでavailability callbackなし、current MediaSyncのfirst successful output callback前はvideo availableにしないこと、old instance callback無視、Surface切替でMediaSync再生成、parental/error gate、AudioTrack attribution、audio output lifetime、subtitle boundary scheduling、audio/video-only、release時pending Tuner ownership回収を含む。試験のqueue数値上限は選択した`ProductProfile`と一致させる。

TvProvider公開モードは `PublishMode` で channel row 追加を setup scan / explicit rescan に限定する。ライブ tune refresh、boot EPG sync、background channel maintenance では既存 channel の番組・診断更新だけを許可し、新規 channel row は追加しない。
'''
text = replace_section(text, "## codec header / A-V sync / publish mode の固定\n", "## ARIB SI/EPG のTvProvider投影\n", codec)

# Synchronize the fixed summary bullet.
old = "- `notifyVideoAvailable()` はcurrent Media3 player/current Surface generationの`Player.Listener.onRenderedFirstFrame()`を受けた後だけ一度呼ぶ。clock進行、decoder output、drop、旧player／旧Surface callbackはavailability確定根拠にしない。物理display/compositor fenceは要求しないが、rendererより前段のcallbackで代用もしない。固定delay、独自clock、独自frame scheduler、hidden API、pixel probeは使わない。"
new = "- `notifyVideoAvailable()` はcurrent MediaSync instanceがlate-drop判定後にcurrent final outputへの最初の`queueBuffer()`成功を確認して発行するFramework-private `@hide` first-output callbackを受けた後だけ一度呼ぶ。decoder output、`OnFrameRendered`、`getTimestamp()`のclock進行、drop、旧MediaSync callbackはavailability確定根拠にしない。物理display/compositor fenceは要求しない。public/System/Test API追加、固定delay、独自A/V clock、独自video frame scheduler、reflection、pixel probeは使わない。"
if text.count(old) != 1:
    raise SystemExit(f"fixed notify bullet count={text.count(old)}")
text = text.replace(old, new, 1)

# Playback executor ownership.
old = "`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。Tuner filter、Media3 MediaSource／SampleStream adapter、ExoPlayer、player generation、Surface generation、pending Tuner sample、budget claim、listener tokenの変更を呼び出し元スレッドで直接行わない。release後のqueued taskはreleased flagとgenerationで破棄する。"
new = "`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。Tuner filter、video/audio MediaCodec、MediaSync、AudioTrack、playback generation、pending Tuner sample、budget claim、first-output listener token、subtitle boundary stateの変更を呼び出し元スレッドで直接行わない。Surface変更はMediaSync再生成を含むpresentation generation変更として同executorで直列化する。release後のqueued taskはreleased flagとgenerationで破棄する。"
if text.count(old) != 1:
    raise SystemExit(f"executor block count={text.count(old)}")
text = text.replace(old, new, 1)

# Tuner callback/backpressure boundary.
old = "- `MediaEvent` sampleは固定4 MiBを上限にしない。負のoffset、0以下のlength、加算overflow、`offset + length > LinearBlock capacity`は不正入力として確保前に破棄する。正常sampleは同一製品profileのper-event予算をclaimしてMedia3 input adapterのpending queueへ渡し、`SampleStream.readData()`時に有効rangeだけを`DecoderInputBuffer.data`へ1回copyする。共有領域方式とイベント固有fd方式を同じpending byte予算へ計上する。\n- Tuner→Media3 input adapterの逆圧は無通知破棄ではない。未読`MediaEvent`／`LinearBlock`は上限付きpending queueとbudget claimに保持し、Media3 `SampleStream.readData()`で消費する。sampleを破棄するのは上限付きqueueが満杯の場合だけとし、破棄counterを加算する。"
new = "- `MediaEvent` sampleは固定4 MiBを上限にしない。負のoffset、0以下のlength、加算overflow、`offset + length > LinearBlock capacity`は不正入力としてqueue前に破棄する。正常sampleは同一製品profileのper-event予算をclaimし、MediaCodec block-model input slot待ちの上限付きpending queueへ保持する。input slot取得時に有効rangeを`QueueRequest.setLinearBlock()`へ直接渡し、ES本体を別bufferへcopyしない。共有領域方式とイベント固有fd方式を同じpending byte予算へ計上する。\n- Tuner→MediaCodecの逆圧は無通知破棄ではない。未queueの`MediaEvent`／`LinearBlock`は上限付きpending queueとbudget claimに保持し、codec input slot取得時に消費する。sampleを破棄するのはqueue上限超過または既存deadline/error規則の場合だけとし、原因別counterを加算する。"
if text.count(old) != 1:
    raise SystemExit(f"backpressure block count={text.count(old)}")
text = text.replace(old, new, 1)

# AttributionSource: direct AudioTrack + MediaSync, no Media3 fork.
pattern = re.compile(r"Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner\(serviceContext, sessionId, useCase\)`へ渡す。audio出力はMedia3が所有するが、.*?session releaseまたはplayer置換後は旧`sessionContext`、旧player、旧AudioSinkを新generationへ再利用しない。", re.S)
replacement = r'''Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。audio出力はTISが生成する`AudioTrack`とMediaSyncで構成し、Android 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`によるTV app attribution chainを失ってはならない。sample rate、channel mask、encoding、buffer size、AudioAttributes等はdecoder output/product profileに従う標準Builder設定とし、生成したAudioTrackを同generationのMediaSyncへ設定する。TISはAudioTrack playback headを独自A/V clockとして扱わず、A/V clockはMediaSyncへ一元化する。通常経路で素の`serviceContext`へ後退せず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。session release、Surface変更またはplayback generation変更後は旧`sessionContext`由来AudioTrack、旧MediaSync、旧decoderを新generationへ再利用しない。'''
text, n = pattern.subn(replacement, text, count=1)
if n != 1:
    raise SystemExit(f"attribution block count={n}")

old = "`setAttributionSource()`を探索・呼出しするreflection、hidden API、vendor独自AIDL、reflection失敗時の無言fallbackを通常経路に置かない。対象system APIを使う必要が生じた場合は、対象SDKへ直接コンパイルできる型付き呼出しとして別途設計する。"
new = "`setAttributionSource()`を探索・呼出しするreflection、vendor独自AIDL、reflection失敗時の無言fallbackを通常経路に置かない。例外は本書で明示したMediaSync first-output用の製品Framework-private `@hide` contractだけとし、TISを`/system_ext`へ置いて同一platform sourceから型付きcompileする。それ以外のnon-SDK APIを便乗して使用してはならない。"
if text.count(old) != 1:
    raise SystemExit(f"hidden api policy count={text.count(old)}")
text = text.replace(old, new, 1)

# Codec table references.
text = text.replace("Media3 Format写像、decoder capability確認、`onRenderedFirstFrame()` gate", "MediaFormat写像、MediaCodec capability確認、MediaSync first-output gate")
text = text.replace("Media3 Format写像 / decoder capability確認 / `onRenderedFirstFrame()` gate", "MediaFormat写像 / MediaCodec capability確認 / MediaSync first-output gate")
text = text.replace("Media3 Format写像 / decoder capability確認 / audio renderer／AudioSink / メタデータ / unsupported 診断情報", "MediaFormat写像 / MediaCodec capability確認 / AudioTrack／MediaSync / メタデータ / unsupported 診断情報")

# No stale Media3 path is allowed to remain in the design after the ownership reversal.
for forbidden in ["Media3", "ExoPlayer", "SampleStream", "CueEncoder", "onRenderedFirstFrame", "AudioTrackProvider"]:
    if forbidden in text:
        raise SystemExit(f"stale playback term remains: {forbidden}")

required = [
    "@hide OnFirstVideoFrameQueuedListener",
    "queueBuffer()",
    "/system_ext",
    "CONFIGURE_FLAG_USE_BLOCK_MODEL",
    "QueueRequest.setLinearBlock",
    "MediaSync.queueAudio",
    "event-driven one-shot subtitle scheduler",
]
for token in required:
    if token not in text:
        raise SystemExit(f"required contract missing: {token}")

p.write_text(text)
