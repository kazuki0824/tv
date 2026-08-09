from pathlib import Path

p = Path("tis/DESIGN_JA.md")
s = p.read_text()


def rep(old: str, new: str, label: str) -> None:
    global s
    count = s.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1, got {count}")
    s = s.replace(old, new, 1)


old = """現行 product の平文 non-tunneled AV入力は、Tuner `MediaEvent.getLinearBlock()` と `MediaCodec` block model の型付き経路だけを正式経路とする。video／audio decoderは `MediaCodec.CONFIGURE_FLAG_USE_BLOCK_MODEL` で構成し、入力slotの `MediaCodec.QueueRequest.setLinearBlock(linearBlock, offset, dataLength)` を使う。`MediaEvent.isPtsPresent()` が true のsampleでは、raw PTSを90 kHz、33 bit modulo値として扱う。`PlaybackPipeline` は playback generation ごとに `PtsNormalizer` を1個だけ所有し、連続するraw PTSの差を modulo `2^33` の最短signed差としてunwrapしてgeneration内のextended PTSへ変換する。extended PTSから `presentationTimeUs = floor(pts90k * 1000000 / 90000)` をoverflow-safeなchecked integer arithmeticで算出し、`MediaCodec.QueueRequest.setPresentationTimeUs()`へ設定する。`isPtsPresent()` が false のsampleはAOSP Tuner API上で表現可能な入力状態として扱い、0、前sample、PCR、wallclock等から時刻を捏造せず、そのsampleだけをcodecへqueueせず解放してtrack別`MISSING_PTS_SAMPLE`診断へ計上する。PTS欠落sample単体を理由にplayback generationを再生不能へ遷移させず、`notifyVideoUnavailable()`も呼ばない。first frame前ではPTS欠落sampleをdecoder入力可能状態への進捗として数えず、既存の`decoderStartupDeadlineMs`を延長またはリセットしない。first frame後も当該sampleだけを破棄し、generation全体のunavailable遷移は既存のdecoder error、lock喪失、startup/backpressure deadline等の独立条件だけで判定する。このsample自体をTuner HALのmalformed eventとは扱わない。

`PtsNormalizer` の状態はretune、新playback generation、filter flush、decoder再生成、非wrap discontinuityで破棄する。通常の33 bit wrapだけは同generation内の連続差としてunwrapし、独自media clock、PCR→wallclock変換、独自future/late schedulerへ拡張しない。reflection、hidden API、`LinearBlock.map()`でESを`ByteArray`へ複製して通常input bufferへ入れる経路、通常ByteBuffer input modelへの代替処理を禁止する。`MediaEvent`、`LinearBlock`、decoder input claimはqueue成功または破棄確定まで保持し、queue成功後にだけ呼出側所有権を解放する。secure `MediaEvent`は現行平文productの対象外とし、mappable blockへの暗黙変換を行わない。"""
new = """現行 product の平文 non-tunneled AV入力は、Tuner `MediaEvent.getLinearBlock()`をTISのMedia3 `MediaSource`／`SampleStream` adapterへ渡す経路を正式経路とする。`MediaEvent.isPtsPresent()` が true のsampleでは、raw PTSを90 kHz、33 bit modulo値として扱う。`PlaybackPipeline` は playback generation ごとに `PtsNormalizer` を1個だけ所有し、連続するraw PTSの差を modulo `2^33` の最短signed差としてunwrapしてgeneration内のextended PTSへ変換する。extended PTSから `presentationTimeUs = floor(pts90k * 1000000 / 90000)` をoverflow-safeなchecked integer arithmeticで算出し、Media3 `DecoderInputBuffer.timeUs`へ設定する。`isPtsPresent()` が false のsampleはAOSP Tuner API上で表現可能な入力状態として扱い、0、前sample、PCR、wallclock等から時刻を捏造せず、そのsampleだけをMedia3へ渡さず解放してtrack別`MISSING_PTS_SAMPLE`診断へ計上する。PTS欠落sample単体を理由にplayback generationを再生不能へ遷移させず、`notifyVideoUnavailable()`も呼ばない。first frame前ではPTS欠落sampleをMedia3入力可能状態への進捗として数えず、既存の`decoderStartupDeadlineMs`を延長またはリセットしない。first frame後も当該sampleだけを破棄し、generation全体のunavailable遷移は既存のplayer／decoder error、lock喪失、startup/backpressure deadline等の独立条件だけで判定する。このsample自体をTuner HALのmalformed eventとは扱わない。

`PtsNormalizer` の状態はretune、新playback generation、filter flush、player／decoder再生成、非wrap discontinuityで破棄する。通常の33 bit wrapだけは同generation内の連続差としてunwrapし、独自media clock、PCR→wallclock変換、独自future/late schedulerへ拡張しない。reflection、hidden API、ES全体の`ByteArray`中継、多重copyを禁止する。`SampleStream.readData()`で要求されたsampleだけを`LinearBlock.map()`でread-only参照し、有効rangeを`DecoderInputBuffer.data`へ1回copyする。`MediaEvent`、`LinearBlock`、input claimはcopy完了または破棄確定まで保持し、copy完了後に呼出側所有権を解放する。secure `MediaEvent`は現行平文productの対象外とし、mappable blockへの暗黙変換を行わない。"""
rep(old, new, "input path")

old = """startup queueと台帳claimを確保した後にAV filterを開始し、上限内の`MediaEvent`からMPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio headerを収集して`MediaFormat`を構成する。header解析に必要な最小範囲だけ`LinearBlock.map()`でread-only参照してよいが、ES本体を`ByteArray`へ複製して通常input bufferへ移送してはならない。decoder構成成功後は同じsnapshotのsteady-state上限へ遷移し、startup queueの`MediaEvent`／`LinearBlock`所有権をblock model QueueRequestへ移す。runtimeで観測したdecoder block capacityは各sampleの投入可否と製品profile検証の診断にだけ用い、開始済み世代のsnapshotまたは予約量を書き換えない。検証済み最小容量を満たさないdecoderではfilterを停止し、claimとHAL handleを解放して`DECODER_CAPACITY_MISMATCH`を記録し、`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。

MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= LinearBlock capacity`を満たす場合だけstartup queueまたはblock model QueueRequestへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。

必要なqueue領域とclaim台帳はplayback generation開始時に原子的に予約する。各eventはrange検証後、block model投入前に`dataLength`をsnapshot台帳へclaimし、いずれかのevent、byte、sample、duration上限を超える場合は原因別に`SAMPLE_TOO_LARGE`または`PENDING_QUEUE_FULL`を記録してHAL handleを解放する。claim済みbyte、sample、durationはQueueRequest成功、破棄、generation変更、stop、releaseで正確に返す。HALの`avPerFilterLiveBytes`または`avRuntimeBudgetBytes`をTISへ公開・複製・1event上限化しない。"""
new = """startup queueと台帳claimを確保した後にAV filterを開始し、上限内の`MediaEvent`からMPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio headerを収集してMedia3 `Format`へ写像する。header解析に必要な最小範囲だけ`LinearBlock.map()`でread-only参照してよいが、ES本体を`ByteArray`へ複製してはならない。必要なformat情報が成立したらMedia3 playerをprepareし、startup queueの`MediaEvent`／`LinearBlock`は`SampleStream.readData()`要求に応じて`DecoderInputBuffer.data`へ1回copyして解放する。decoder capability不足またはplayerのdecoder初期化失敗は`DECODER_CAPACITY_MISMATCH`または`UNSUPPORTED_*_CODEC`の型付き診断に落とし、filter、pending sample、claim、Media3 playerを回収して`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。

MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= LinearBlock capacity`を満たす場合だけstartup queueまたはMedia3 input adapterのpending queueへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。

必要なqueue領域とclaim台帳はplayback generation開始時に原子的に予約する。各eventはrange検証後、Media3 input adapterへenqueueする前に`dataLength`をsnapshot台帳へclaimし、いずれかのevent、byte、sample、duration上限を超える場合は原因別に`SAMPLE_TOO_LARGE`または`PENDING_QUEUE_FULL`を記録してHAL handleを解放する。claim済みbyte、sample、durationは`SampleStream.readData()`へのcopy完了、破棄、generation変更、stop、releaseで正確に返す。HALの`avPerFilterLiveBytes`または`avRuntimeBudgetBytes`をTISへ公開・複製・1event上限化しない。"""
rep(old, new, "budget transfer")

rep(
    "- `MediaEvent` sampleは固定4 MiBを上限にしない。負のoffset、0以下のlength、加算overflow、`offset + length > LinearBlock capacity`は不正入力として確保前に破棄する。正常sampleはES全体をcopyせず、同一製品profileのper-event予算をclaimしてblock model QueueRequestへ渡す。共有領域方式とイベント固有fd方式を同じpending byte予算へ計上する。",
    "- `MediaEvent` sampleは固定4 MiBを上限にしない。負のoffset、0以下のlength、加算overflow、`offset + length > LinearBlock capacity`は不正入力として確保前に破棄する。正常sampleは同一製品profileのper-event予算をclaimしてMedia3 input adapterのpending queueへ渡し、`SampleStream.readData()`時に有効rangeだけを`DecoderInputBuffer.data`へ1回copyする。共有領域方式とイベント固有fd方式を同じpending byte予算へ計上する。",
    "callback sample",
)

old = """Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。AudioTrack生成はAndroid 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`を必須とし、`sessionContext.getAttributionSource()`からTV app attribution chainとdevice固有audio session情報を伝播させる。通常経路で素の`serviceContext`をAudioTrackへ渡さず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。現行playbackではTISがAudioTrackを直接生成せず、`ExoPlayer.Builder(sessionContext)`で同generationのMedia3 playerを生成し、その標準audio renderer／AudioSinkにaudio出力を所有させる。session releaseまたはplayer置換後は旧`sessionContext`と旧playerを新generationへ再利用しない。"""
new = """Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。audio出力はMedia3が所有するが、Android 14（API 34）の`AudioTrack.Builder.setContext(sessionContext)`によるTV app attribution chainを失ってはならない。現行productでは`DefaultRenderersFactory`の`buildAudioSink(...)`をoverrideし、`DefaultAudioSink`へ`AudioTrackAudioOutputProvider`を供給して、その公開`setAudioTrackBuilderModifier(...)`で生成される各`AudioTrack.Builder`へ`setContext(sessionContext)`を設定する。これによりdecoder／clock／AudioSinkの所有はMedia3に残したままAudioTrack attributionをsession固有Contextへ固定する。通常経路で素の`serviceContext`へ後退せず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。session releaseまたはplayer置換後は旧`sessionContext`、旧player、旧AudioSinkを新generationへ再利用しない。"""
rep(old, new, "attribution graph")

p.write_text(s)
