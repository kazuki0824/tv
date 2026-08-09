from pathlib import Path
import re

p = Path('tis/DESIGN_JA.md')
text = p.read_text(encoding='utf-8')

def replace_once(old: str, new: str) -> None:
    global text
    n = text.count(old)
    if n != 1:
        raise SystemExit(f'expected one match, got {n}: {old[:120]!r}')
    text = text.replace(old, new, 1)

old = """video decoderには実行開始前に `MediaCodec.OnFrameRenderedListener` を登録する。codecのoutput Surfaceは`MediaSync.createInputSurface()`で得たMediaSync入力Surfaceなので、current decoder/current playback generationの最初の`onFrameRendered()`はdecoderからMediaSync入力へ少なくとも1 frameが渡ったことを示す中間観測に限定し、その`presentationTimeUs`をcurrent generationの`firstFrameCandidatePtsUs`として保持する。Android公開`MediaSync` APIには、その後の特定video frameが`sessionSurface`へ実際に提示されたことを通知するvideo presentation callbackはなく、`MediaSync.getTimestamp()`も表示済みvideo frameのcommit通知ではないため、厳密なcompositor presentation証明には使わない。一方で`MediaSync.getTimestamp()`はcurrent playback positionを返す公開APIなので、future PTSのcandidateをpresentation時刻より前にavailable扱いしないtemporal gateとして使用する。`notifyVideoAvailable()`は、candidateが存在し、MediaSyncがcurrent generationでplayback rate 0より大きく開始済みで、`getTimestamp()`がnon-nullかつ`MediaTimestamp.getMediaClockRate()>0`で、そのcurrent playback positionである`getAnchorMediaTimeUs()`が`firstFrameCandidatePtsUs`以上に到達し、`MediaSync.setSurface()`へ設定したcurrent `sessionSurface`が有効、視聴制限でblockされておらず、同generationで`MEDIASYNC_ERROR_SURFACE_FAIL`が発生していない場合だけ一度呼ぶ。`getTimestamp()`がnullまたはcandidate PTS未到達なら通知せず、後続のcurrent-generation decoder/MediaSync callbackで同じgateを再評価する。このtimestamp gateはcandidateのdue time到達前通知を防ぐためのbest-effort条件であり、実際のoutput Surface/compositor提示完了を証明するものとは扱わない。旧decoder/旧generationのcallbackは無視する。固定delay、独自clock、独自frame scheduler、hidden API、Surface/compositor pixel probeは追加しない。"""
new = """### MediaSync Framework-private final-output observation

stock Android 14 / LineageOS 21 の `MediaSync` はvideo scheduling/dropをnative側で所有し、late frameをinputへ返すdrop分岐と、render対象frameをcurrent outputへattachして`queueBuffer()`する分岐を区別する。一方、公開Java APIにはそのfinal-output成功をvideo clientへ通知するcallbackがない。この不足だけを閉じるため、対象LineageOS platformの`android.media.MediaSync`へ、既存public `MediaSync.Callback`とは別の `@hide OnFirstVideoFrameQueuedListener` と `@hide setOnFirstVideoFrameQueuedListener(listener, handler)` 相当を追加する。public SDK、`@SystemApi`、`@TestApi`、Tuner AIDL/VINTFは変更しない。

non-null listenerを設定する操作は、current MediaSync instanceの**次のfinal-output成功を1件だけ通知するavailability epochをarmする操作**でもある。native MediaSyncは、arm中にvideo bufferがlate-drop分岐を通過し、current `mOutput`へのattachと`queueBuffer()`がともに成功した後だけeventを1件生成し、そのepochをdisarmする。late-drop、attach失敗、queue失敗、output abandonment、inputへ返したbufferではeventを生成せず、arm状態も消費しない。常時すべてのrender成功を通知せず、TISが必要なavailability epochだけをarm/re-armする。

TISは新playback generation開始時にinitial availability epochをarmする。current instance/current generationのeventを受け、current `sessionSurface`が有効、視聴制限でblockされておらず、同generationの`MEDIASYNC_ERROR_SURFACE_FAIL`がない場合だけ`notifyVideoAvailable()`を呼ぶ。一度availableになった後、`VIDEO_UNAVAILABLE_REASON_BUFFERING`等のrecoverable unavailableへ遷移し**同じMediaSync instance/generationを維持して復旧する場合**は、復旧開始時にlistenerを再設定して次のfinal-output成功をre-armし、その成功event後だけ再びavailableへ遷移する。generation teardownを伴うunavailableは新MediaSync instanceのinitial armで閉じる。これにより`available -> unavailable -> available`をrecoverable pathとgeneration-recreate pathの双方で閉じる。

callbackは物理display/compositorへのpresent fence完了を意味せず、video scheduling/drop ownerであるMediaSyncがrender対象を選択しcurrent final outputへのqueueを成功させたことだけをcommitする。`MediaCodec.OnFrameRenderedListener`、`MediaSync.getTimestamp()`、playback clock進行はvideo availability commitへ使用しない。native内部mutexを保持したままJavaへreentrant callせず、JNI/Java handlerへ非同期配送する。release済みまたは旧generation/旧MediaSync instanceから遅延配送されたeventはstate更新に使わない。

このcallbackは同一製品buildでFrameworkと同時更新されるplatform-private contractである。TIS APKは`/system_ext`のplatform-coupled componentとして同一platform sourceに対して型付きcompileし、reflection、hidden API allowlist回避、callback不存在時のtimestamp推測fallbackを置かない。Framework patchを持たないbuildは現行product playback contractを満たさないためintegration/build時に拒否する。"""
replace_once(old, new)

replace_once(
"""`tunneled`／platform passthrough playback pathは現行productの設計候補から外し、実装しない。`notifyVideoAvailable()`は、本書「再生経路」で定義したとおり、current decoder/current generationの最初の`MediaCodec.OnFrameRenderedListener.onFrameRendered()`をMediaSync入力到達の中間観測としてcandidate PTSを保持し、`MediaSync.getTimestamp()`のcurrent playback positionがそのcandidate PTSへ到達したことをfuture-frame早期通知防止のtemporal gateとして確認する。さらにMediaSync再生開始、current session Surface有効、generation、視聴制限、MediaSync Surface errorの各gateを満たした場合だけ通知する。`MediaSync.getTimestamp()`を表示済みvideo frameの証明には使わず、単なるfilter開始、入力event到着、decoder output生成だけでも通知しない。""",
"""`tunneled`／platform passthrough playback pathは現行productの設計候補から外し、実装しない。`notifyVideoAvailable()`は、本書「MediaSync Framework-private final-output observation」で定義したcurrent availability epochのfinal-output成功eventだけをcommitにする。initial generationおよび同instanceでrecoverable unavailableから復旧するepochごとにlistenerをarm/re-armし、decoder output、`OnFrameRendered`、`getTimestamp()`のclock進行だけでは通知しない。"""
)

replace_once(
"""- `notifyVideoAvailable()` は `MediaCodec.OnFrameRenderedListener.onFrameRendered()`をMediaSync入力Surface到達の中間観測としてcandidate PTSを保持し、`MediaSync.getTimestamp()`のcurrent playback positionがcandidate PTS以上に到達するまで通知しない。timestampはfuture-frame早期通知防止のtemporal gateに限定し、最終`sessionSurface`提示完了の証明とは扱わない。加えてplayback rateが0より大きく、current session Surfaceが有効で`MEDIASYNC_ERROR_SURFACE_FAIL`がなく、視聴制限でblockされていない場合に一度だけ呼ぶ。固定delay、独自clock、hidden API、pixel probeは使わない。""",
"""- `notifyVideoAvailable()` はcurrent MediaSync availability epochでlate-dropを通過しcurrent final outputへのattach＋`queueBuffer()`成功後に発行されるFramework-private first-output eventを受け、current Surface有効、generation一致、視聴制限、Surface errorのgateを満たした場合だけ呼ぶ。recoverable unavailableから同MediaSync instanceで復旧する場合はlistenerをre-armし、次の成功event後に再度availableへ遷移する。decoder output、`OnFrameRendered`、`getTimestamp()`のclock進行、drop、旧instance callbackはavailability根拠にしない。"""
)

replace_once(
"""`setAttributionSource()`を探索・呼出しするreflection、hidden API、vendor独自AIDL、reflection失敗時の無言fallbackを通常経路に置かない。対象system APIを使う必要が生じた場合は、対象SDKへ直接コンパイルできる型付き呼出しとして別途設計する。""",
"""`setAttributionSource()`を探索・呼出しするreflection、vendor独自AIDL、reflection失敗時の無言fallbackを通常経路に置かない。例外は本書で明示したMediaSync final-output観測用の製品Framework-private `@hide` contractだけとし、TISを`/system_ext`へ置いて同一platform sourceから型付きcompileする。それ以外のnon-SDK APIを便乗して使用してはならない。"""
)

old_test = """最低試験契約は、型付きLinearBlock→block model QueueRequest、ES全体copy禁止、PTS欠落sample単体dropとgeneration継続、通常PTS wrap、`OnFrameRendered`単独ではvideo availableにしないこと、candidate PTSに対する`MediaSync.getTimestamp()`のcurrent playback positionが未到達またはnullの間はfuture-frame早期通知を抑止すること、timestampをfinal video presentation証明とは扱わないこと、candidate PTS到達後もplayback rate／current Surface／parental／Surface-error gateが揃うまで抑止し条件成立後に一回だけavailability通知すること、audio bufferのconsume callbackまでの寿命、A/V同期、video-only、audio-only、非wrap discontinuity後のMediaSync再生成、Surface／route変更後の旧generation非利用、MediaSync error写像、stale generation非描画を含む。試験のqueue数値上限は選択した`ProductProfile`と一致させる。"""
new_test = """最低試験契約は、型付きLinearBlock→block model QueueRequest、ES全体copy禁止、PTS欠落sample単体dropとgeneration継続、通常PTS wrap、MediaSync rate-0有限prefillからstartup gate成立後speed 1.0へ遷移すること、`MEDIASYNC_ERROR_SURFACE_FAIL`と`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`の分離、audio-videoでAudioTrack failure時のvideo-only継続、audio-onlyでの再生不能遷移、video-onlyがAudioTrack error遷移を持たないこと、initial availability epochでfinal-output成功前はvideo availableにしないこと、late-drop／attach失敗／queue失敗でepochを消費しないこと、成功event後だけ一回availability通知すること、`available -> recoverable unavailable -> available`で同MediaSync instanceを維持する場合にlistenerをre-armして次のfinal-output成功後だけ再availableにすること、generation teardown後は新instanceのinitial armを使うこと、audio bufferのconsume callbackまでの寿命、A/V同期、video-only、audio-only、retune／flush／非wrap discontinuity／Surface変更／AudioTrack切替・再生成／audio route変更／decoder再生成後のMediaSync再生成、route変更後の旧generation非利用、MediaSync error写像、stale generation非描画を含む。試験のqueue数値上限は選択した`ProductProfile`と一致させる。"""
replace_once(old_test, new_test)

marker = '## ライブ playback 実装方式\n'
subtitle = """### 字幕PTS scheduling / clear ownership

字幕のmedia clockはvideo/audioと同じcurrent MediaSyncのcanonical clockだけとする。TIS、Rust JNI、`CaptionOverlayView`はPCR/wallclockから別media clockを作らず、固定delayや周期的な`getTimestamp()` polling loopも持たない。ARIB字幕PESから得たPTSはvideo/audioと同じ33-bit 90 kHz unwrap規則でcurrent playback generationの`timeUs`へ変換し、libaribcaption decoder/rendererへ渡す。rendererが返したRGBA8888画像とrender regionはin-processでoverlay所有のBitmap等へ直接受け渡しし、serialization round-tripを挟まない。

字幕display/clearは、MediaSyncの`getTimestamp()`が返すmedia time / anchor time / playback rateを唯一の時間基準とするevent-driven one-shot subtitle schedulerが担当する。新caption、finite-durationのclear、明示clearのうち次の1境界だけをarmし、予定時刻到達時にcurrent MediaSync timestampを再読して境界到達を確認する。まだearlyなら同じ境界へre-armし、dueならdisplay/clearして次境界だけをarmする。周期polling、独立free-running clock、PCR→wallclock clock、video frame release/drop判定を実装しない。playback rate変更、flush、retune、Surface/MediaSync generation変更、track disable、session releaseではpending subtitle eventをcancel/re-armする。

libaribcaptionが有限`wait_duration`を返すcaptionは`PTS + duration`で明示clearを予約する。`DURATION_INDEFINITE`は有限値へ推測変換せず、次caption、ARIB/libaribcaptionの明示clear、字幕track無効化、generation終了までcurrent imageを保持する。次captionはそのPTS境界で旧imageを直接replaceする。既に表示済みcaptionのdurationを後から別clockで補正しない。`onSelectTrack(TYPE_SUBTITLE, null)`、retune、Surface/session release、playback generation変更は即時にscheduler stateとoverlayをclearし、旧generationのlibaribcaption結果/eventはgeneration tokenで破棄する。

このschedulerはA/V clockやvideo schedulerを複製するものではなく、MediaSyncが所有するcanonical playback positionに字幕presentation eventを従属させるUI dispatch層である。libaribcaption rendererのRGBA8888出力はこのin-process overlay経路で表示する。

"""
if marker not in text:
    raise SystemExit('live playback marker missing')
text = text.replace(marker, subtitle + marker, 1)

for forbidden in ['Media3', 'ExoPlayer', 'SampleStream', 'CueEncoder', 'onRenderedFirstFrame', 'AudioTrackProvider']:
    if forbidden in text:
        raise SystemExit(f'review-history residue remains: {forbidden}')
for required in [
    'MediaSyncは生成時のplayback rate 0を用いて必要な有限prefillを行い',
    'MEDIASYNC_ERROR_SURFACE_FAIL',
    'MEDIASYNC_ERROR_AUDIOTRACK_FAIL',
    'AudioTrack切替／再生成、audio route変更、decoder再生成',
    '@hide OnFirstVideoFrameQueuedListener',
    'availability epoch', '/system_ext', 'DURATION_INDEFINITE'
]:
    if required not in text:
        raise SystemExit(f'required contract missing: {required}')

p.write_text(text, encoding='utf-8')
