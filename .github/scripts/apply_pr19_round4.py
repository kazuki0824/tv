from pathlib import Path

p=Path('tis/DESIGN_JA.md')
s=p.read_text()
def rr(a,b,r):
    global s
    assert s.count(a)==1,(a,s.count(a))
    i=s.index(a);j=s.index(b,i);s=s[:i]+r+s[j:]

rr('デコード後のA/V同期とSurface提示はAndroid標準`MediaSync`だけを使用する。','\n\n## EIT と TvProvider','''デコード後のA/V同期とSurface提示は、Android platformまたはAndroidXの標準renderer/player機構に委ねる。TIS自身は独自media clock、`AudioTimestamp`由来の独自同期、独自future render／late drop scheduler、固定delayを持たない。video側には設計上の役割として`VideoPresentationRenderer`境界を置く。この境界は特定ライブラリのクラス名ではなく、decoderからcurrent `sessionSurface`までのvideo scheduling／dropを最終的に所有し、dropしたframeとcurrent output Surfaceへrenderしたframeを区別できるrenderer契約を表す。最終rendererは、current playback generationかつcurrent Surface generationの非drop frameをcurrent `sessionSurface`へrenderした後にだけfirst-frame-rendered eventを返す。このeventは物理display/compositorのpresentation fenceを意味せず、TIFが要求する「content rendered onto its surface is ready for viewing」を判定するrenderer境界のcommitとする。

`MediaCodec.OnFrameRenderedListener`は、codecのoutput Surface自体がcurrent `sessionSurface`であり、その後段にvideo scheduling／dropを行う層が存在しない構成でのみfinal-renderの根拠にできる。codec outputが`MediaSync.createInputSurface()`で得たMediaSync入力Surfaceである構成では、`onFrameRendered()`はdecoderからMediaSync入力への到達を示す中間観測に限定する。`MediaSync.getTimestamp()`はcurrent playback positionの観測に過ぎず、MediaSync内部で当該video frameがrenderされたかdropされたかを区別できないため、first-frame availabilityの確定根拠または代替commitには使わない。公開`MediaSync` APIだけでMediaSync出力側のrender/dropを区別するfirst-frame eventを取得できない構成は、現行productの完成したvideo availability経路として採用しない。

適合する標準renderer構成は、最終output Surfaceへのfirst-frame-rendered eventとdrop eventを分離して公開し、A/V同期とframe release schedulingをその標準renderer/player側が所有することを必須とする。Media3を採用する場合は、`VideoSink.Listener.onFirstFrameRendered()`／`onFrameDropped()`相当を最終renderer eventとして使い、frame schedulingをTIS独自loopで再実装せずMedia3 renderer側に所有させる。AOSP Live TV系のように最終Surfaceへのdraw完了をrendererからsessionへ返す構成も同じ契約を満たし得る。特定renderer製品の採用自体は本設計では固定せず、上記境界を満たすAndroid標準経路であることを固定する。

`notifyVideoAvailable()`は、current playback generation/current Surface generationに結び付いた最終rendererのfirst-frame-rendered eventを受け、current `sessionSurface`が有効、視聴制限でblockされておらず、同generationのrenderer／Surface failureがない場合だけ一度呼ぶ。frame-available-before-render、decoder output生成、MediaSync入力到達、media clockのcandidate PTS到達、drop eventだけでは通知しない。旧generation／旧Surfaceのcallbackは無視する。固定delay、独自clock、独自frame scheduler、hidden API、Surface/compositor pixel probeは追加しない。audio bufferの所有権と寿命は選択した標準renderer/playerの公開契約に従う。''')

rr('## ライブ playback 実装方式\n\n','\n\nsetup scan の channel registration','''## ライブ playback 実装方式

TIS のライブplaybackは、Tuner AV filterの平文`MediaEvent.LinearBlock`をMediaCodec block modelへcopyなしで投入する入力契約を維持する。decoder以後のA/V同期、video scheduling／drop、current `sessionSurface`への提示は、本書「再生経路」の`VideoPresentationRenderer`境界を満たすAndroid platform／AndroidX標準renderer/playerへ委ねる。TISはその外側に独自clockまたは独自frame schedulerを置かない。codec outputを`MediaSync.createInputSurface()`へ入れ、MediaSync前段の`OnFrameRendered`と`getTimestamp()`だけで最終availabilityを確定する構成は採用しない。

`tunneled`／platform passthrough playback pathは現行productの設計候補から外し、実装しない。`notifyVideoAvailable()`は、video scheduling／dropを最終的に所有するrendererがcurrent generation/current `sessionSurface`へ非dropのfirst frameをrenderしたeventを唯一のvideo成功commitとして扱い、current Surface有効、generation一致、視聴制限、renderer／Surface errorの各gateを満たした場合だけ一度通知する。frame available、decoder output、MediaSync入力到達、`MediaSync.getTimestamp()`のmedia position到達、drop eventをfinal commitへ昇格させない。''')

rr('A/V同期方式は現行productでAndroid標準`MediaSync`に固定する。','\n\nTvProvider公開モードは','''A/V同期方式は現行productでAndroid platformまたはAndroidXの標準renderer/player機構に固定し、TIS独自のmedia clock、frame release clock、future/late判定を実装しない。`PlaybackPipeline`のserial executorはcurrent playback generation、current Surface generation、選択した標準renderer/player、session Surface、video／audio decoder、未返却bufferとbudget claimを単一所有し、decoder／renderer／audio route callbackはstateを直接変更せず同executorへ直列化する。

video経路の必須境界を`VideoPresentationRenderer`とする。これはvideo scheduling／dropを最終的に決定する層であり、current `sessionSurface`をoutputとして所有し、少なくともfirst-frame-rendered、frame-dropped、surface/render errorを区別してTISへ返す。first-frame-renderedは非drop frameをcurrent output Surfaceへrenderした後にだけ発火する契約とし、物理display/compositor fenceまでは要求しない。listener instanceまたはcallback tokenをplayback generationとSurface generationへ結び付け、retune、Surface変更、decoder／renderer再生成後のstale callbackは状態更新に使わない。

`MediaCodec.OnFrameRenderedListener`を使う場合、codec output Surfaceが最終current `sessionSurface`で、その後段に別のscheduling／drop所有者が存在しないことをfinal event採用条件とする。codec outputが`MediaSync.createInputSurface()`である場合、同callbackは中間観測でありavailabilityには使わない。`MediaSync.getTimestamp()`もmedia position観測であってrender/drop結果ではないためavailabilityのcommitに使わない。MediaSyncをvideo schedulingの最終所有者にするだけの構成は、公開APIで最終outputへのfirst renderとdropを区別できないため本契約を満たさない。

適合rendererの具体製品は固定しない。Media3等を採用する場合はfinal rendererのfirst-frame-rendered eventとdrop eventを区別し、frame release schedulingを標準renderer側に所有させる。別のAndroid標準renderer/playerを採用する場合も同じfirst-render/drop/error契約を満たすことを必要条件とする。

audio経路も選択した標準renderer/playerのA/V同期機構へ委ねる。buffer ownershipはその公開契約に従い、TIS独自clockでvideoへ同期させない。video-onlyではaudio rendererを作らず、audio-onlyでは`VideoPresentationRenderer`を作らない。

retune、playback generation変更、stop、flush、非wrap PTS discontinuity、Surface変更、audio route変更、decoder／renderer再生成では旧rendererのgeneration／Surface tokenを失効させ、未返却bufferと旧decoder outputを回収して新generationとして標準renderer/playerを再生成または再bindする。通常33-bit PTS wrapはgeneration内でunwrapし、wrapだけではgenerationを変更しない。旧generation／旧Surfaceのrenderer callbackはstate更新に使わない。

最低試験契約は、型付きLinearBlock→block model QueueRequest、ES全体copy禁止、PTS欠落sample単体dropとgeneration継続、通常PTS wrap、decoder outputまたはMediaSync入力到達だけではvideo availableにしないこと、media position到達だけではvideo availableにしないこと、final rendererがdropしたframeでは通知しないこと、current generation/current Surfaceへのfirst-frame-rendered eventを受けるまで抑止すること、event後もSurface／parental／renderer-error gateが揃うまで抑止し条件成立後に一回だけavailability通知すること、stale generation／stale Surface callback非採用、audio buffer ownership、A/V同期、video-only、audio-only、非wrap discontinuity後のrenderer再生成、Surface／route変更後の旧generation非利用、renderer error写像を含む。試験のqueue数値上限は選択した`ProductProfile`と一致させる。''')

rr('- `notifyVideoAvailable()` は `MediaCodec.OnFrameRenderedListener.onFrameRendered()`をMediaSync入力Surface到達の中間観測としてcandidate PTSを保持し、','\n- ライブ tune refresh','''- `notifyVideoAvailable()` は、video scheduling／dropを最終的に所有する`VideoPresentationRenderer`がcurrent playback generation/current Surface generationの非drop first frameをcurrent `sessionSurface`へrenderしたeventを受けた後だけ一度呼ぶ。`MediaCodec.OnFrameRenderedListener`がMediaSync入力Surface到達を示すだけの構成、`MediaSync.getTimestamp()`のmedia position、frame-available event、drop eventはavailability確定根拠にしない。物理display/compositor fenceは要求しないが、最終rendererより前段のcallbackで代用もしない。固定delay、独自clock、独自frame scheduler、hidden API、pixel probeは使わない。''')

p.write_text(s)
