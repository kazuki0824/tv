from pathlib import Path


def replace_exact(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}")
    path.write_text(text.replace(old, new), encoding="utf-8")


path = Path("tis/DESIGN_JA.md")

old_sync = """A/V同期方式は現行 product で non-tunneled 平文視聴に固定する。tunneled playback と avSyncHwId は TIS non-tunneled playback 範囲外であり、TIS の non-tunneled playback では avSyncHwId を使わないが、Tuner HAL API としては実装・AOSP準拠に仕様を固定する。video/audio は MediaCodec と AudioTrack の PTS により同期する。audio が存在する場合は AudioTrack を master clock とする。video-only サービスは視聴可能として扱い、audio が存在しない場合は `audio absent`、audio codec だけが未対応の場合は `unsupported audio codec` を診断に残す。"""
new_sync = """A/V同期方式は現行 product で non-tunneled 平文視聴に固定する。tunneled playback と avSyncHwId は TIS non-tunneled playback 範囲外であり、TIS の non-tunneled playback では avSyncHwId を使わないが、Tuner HAL API としては実装・AOSP準拠に仕様を固定する。直接MediaCodec／AudioTrack経路のmedia clockと映像描画判断は`PlaybackPipeline`のserial executorが単一所有し、decoder callback、AudioTrack callback、route callbackがclock anchorまたはSurface描画を直接確定してはならない。

音声再生中は`AudioPlaybackClock`を唯一のmaster clockとする。`AudioPlaybackClock`は、現playback generationのaudio PTS anchor、AudioTrackのsample rate、`AudioTimestamp.framePosition`とmonotonic `nanoTime`の対応を同一snapshotとして保持し、32-bit frame positionをgeneration内でunwrapして単調なmedia positionを算出する。新しい有効timestampでのみanchorを更新し、timestamp取得不能またはroute切替中に旧anchorを無期限利用しない。再anchorまでの保持は既存の有限startup／steady-state期限へ従い、audioが存在する世代を黙示的にvideo-only clockへ切り替えない。

`VideoFrameScheduler`は、現generationのvideo PTSとmaster clockのmedia positionとの差から、future frameの有限保持、`MediaCodec.releaseOutputBuffer(index, renderTimestampNs)`による時刻指定描画、late frameの非描画解放を決定する唯一の所有者とする。decoder output callbackはbuffer index、PTS、generationをschedulerへ渡すだけとし、直接renderしない。future／late判定閾値、最大保持時間、連続drop診断閾値はcodec・decoder・device別の有限`ProductProfile`値とし、既存のqueue byte／sample／duration予算を超えてframeを保持しない。

有効なAudioTrackが存在しないvideo-only世代では`StandalonePlaybackClock`を使用し、最初に受理したvideo PTSと`System.nanoTime()`を対応付けて単調なmedia positionを生成する。通常の33-bit PTS wrapはgeneration内でunwrapし、wrapだけをdiscontinuity扱いしない。wrapでは説明できないPTS discontinuityは旧anchorを失効させ、次の有効sampleから再anchorする。

retune、playback generation変更、stop、flush、AudioTrack切替または再生成、audio route変更、decoder再生成では関連clock anchorと予約済みrender時刻を失効させる。Surface変更では旧Surface向け未描画bufferを非描画解放してschedulerのrender targetを更新する。旧generationのAudioTimestamp、decoder output、route callbackはmedia positionまたは描画判断へ使用せず、旧output bufferは非描画解放する。このTIS側clockはHALのPCR clockおよび`future_work/not_planned/avsync_wallclock_research.md`とは別の所有境界である。

最低試験契約は、audio masterでの単調media positionと時刻指定描画、video-only clock、通常PTS wrap、非wrap discontinuity後の再anchor、audio route変更後の旧anchor非利用、stale generation非描画、future frame保持、late frame dropを含む。試験の数値閾値は選択した`ProductProfile`と一致させる。video-only サービスは視聴可能として扱い、audio が存在しない場合は `audio absent`、audio codec だけが未対応の場合は `unsupported audio codec` を診断に残す。"""
replace_exact(path, old_sync, new_sync)

old_session = """- Android 14 系の通常 ライブセッション 生成では `onCreateSession(inputId, sessionId)` を実装し、framework 由来 `sessionId` を `Tuner(context, sessionId, useCase)` へ渡す。1引数 overload の 代替処理 sessionId は互換経路専用とする。"""
new_session = """- LineageOS 21／Android 14の通常ライブセッション生成では`onCreateSession(inputId, sessionId, tvAppAttributionSource)`をoverrideする。framework由来`sessionId`は`Tuner(serviceContext, sessionId, useCase)`へ渡し、`tvAppAttributionSource`はsession固有Contextの生成へ渡す。2引数版`onCreateSession(inputId, sessionId)`と1引数版は明示的な互換経路だけに限定し、対象productの通常3引数入口を素のservice Contextへ委譲または後退させない。"""
replace_exact(path, old_session, new_session)

old_attr = """`AttributionSource?` は `TvInputService.onCreateSession(..., AttributionSource)` から、Tuner SDKなど型付きAPIが要求する境界まで保持して渡す。AudioTrack生成ではAndroid 14（API 34）の公開`AudioTrack.Builder.setContext(Context)`へ`TvInputService`のnon-null `Context`を直接設定し、ContextからAttributionSourceとdevice固有audio session情報を伝播させる。`setAttributionSource()`を探索・呼出しするreflection、hidden API呼出し、reflection失敗時の無言fallbackを通常経路に置かない。対象system APIを使う必要が生じた場合は、対象SDKへ直接コンパイルできる型付き呼出しとして別途設計する。"""
new_attr = """LineageOS 21の通常経路では、`TvInputService.onCreateSession(inputId, sessionId, tvAppAttributionSource)`で受け取ったnon-null `tvAppAttributionSource`をsession寿命中のattribution正本とする。session生成時に`serviceContext.createContext(new ContextParams.Builder().setNextAttributionSource(tvAppAttributionSource).build())`で変更不能なsession固有`sessionContext`を作り、`sessionId`、`tvAppAttributionSource`、`sessionContext`を同じsession creation snapshotへ確定する。途中失敗ではSessionを公開せず、作成済みartifactを解放する。

Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。AudioTrack生成はAndroid 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`を必須とし、`sessionContext.getAttributionSource()`からTV app attribution chainとdevice固有audio session情報を伝播させる。通常経路で素の`serviceContext`をAudioTrackへ渡さず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。session releaseまたは置換後は旧`sessionContext`を新しいAudioTrack生成へ再利用しない。

`setAttributionSource()`を探索・呼出しするreflection、hidden API、vendor独自AIDL、reflection失敗時の無言fallbackを通常経路に置かない。対象system APIを使う必要が生じた場合は、対象SDKへ直接コンパイルできる型付き呼出しとして別途設計する。"""
replace_exact(path, old_attr, new_attr)

text = path.read_text(encoding="utf-8")
required = [
    "onCreateSession(inputId, sessionId, tvAppAttributionSource)",
    "ContextParams.Builder().setNextAttributionSource(tvAppAttributionSource)",
    "AudioTrack.Builder.setContext(sessionContext)",
    "`AudioPlaybackClock`を唯一のmaster clock",
    "`VideoFrameScheduler`は",
    "`StandalonePlaybackClock`を使用",
    "通常PTS wrap",
    "stale generation非描画",
]
for needle in required:
    if needle not in text:
        raise SystemExit(f"missing required text: {needle}")

forbidden = [
    "通常 ライブセッション 生成では `onCreateSession(inputId, sessionId)` を実装",
    "`TvInputService`のnon-null `Context`を直接設定",
    "audio が存在する場合は AudioTrack を master clock とする。",
]
for needle in forbidden:
    if needle in text:
        raise SystemExit(f"stale text remains: {needle}")

if text.count("```") % 2:
    raise SystemExit("unbalanced Markdown code fences")
