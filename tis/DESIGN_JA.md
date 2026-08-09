# TIS 設計判断

## AOSP 標準経路

TIS は `TvInputService` として システムTVアプリ から呼ばれ、Tuner HAL には Tuner SDK API 経由でアクセスする。HAL binder を直接呼ばない。
TIS の setup / boot EPG sync / user unlock drain は、固定文字列や package 名を inputId とみなしてはならない。`TvInputManager.tvInputList` から自 `MaleicacidTvInputService` に一致する `TvInputInfo.id` を一意に解決し、その inputId だけを scan / sync / TvProvider writer へ渡す。解決不能または複数一致の場合、boot EPG sync は pending のまま延期し、setup scan は開始しない。

## BS と CS110 の選局契約

BSはIF周波数とAOSP Tuner公開契約のtyped stream selectorを保持する。通常のscan候補、channel保存、再選局ではbackend種別に依存せず、`STREAM_ID`のTSID `0..65534`だけを使用する。TISはpx4の相対slot、Linux DVBの`DTV_STREAM_ID`、HAL内部のbackend capabilityを取得・推測・保存しない。CS110は周波数帯だけでscan candidateとtune selectorを作り、stream selectorを保存しない。

CS110のTIS内部モデルとTvProvider保存形式では、frontend stream selectorを`None`／`null`として保持する。Android 14 Tuner API builderへ変換するときは`streamId`と`streamIdType`のsetterをどちらも呼ばない。builderが生成する`STREAM_ID`と`INVALID_STREAM_ID(0xFFFF)`の組を、Tuner HALが公開契約境界で`NoSelector`へ正規化する。TISから`UNDEFINED`、0、TSID、relative番号を「selectorなし」の代用として明示設定しない。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BSの通常製品経路はIF周波数と`STREAM_ID`のTSIDを使う。TISはdriver固有slotへ変換せず、typed selectorの検証とbackend ABIへの写像はTuner HALへ委ねる。

TvProvider の channel internal provider data には JSON v1 `tune.streamIdType` と `tune.streamId` を保存する。通常製品経路で書き込む値は、`NONE` の `streamId=null`、または `TSID` の `0..65534`だけとする。`65535`はAOSP `INVALID_STREAM_ID`であり、実TSIDとして保存または再投入しない。`RELATIVE`はdriver固有値になるため、TISの通常channelデータへ保存しない。


## 製品 scan 候補表の保持者

製品scanの選局対象、周波数帯、CATV中心周波数、VHF除外、BS/CS110 selector境界を含む規範値は、tv直下の`開発規則.md`の「製品 scan 候補の規範値」を唯一の設計正本とする。

TISの候補表は製品scan実装データのSSOTであり、`開発規則.md`の規範値に従うscan候補の実装データを唯一保持する。実行時にexplicit tune candidateを生成し、Tuner HALへ渡すscan値はTISが生成したexplicit tune candidateに限定する。TIS以外の文書や実装に同等の候補表を重複保持せず、Tuner HALは日本向けscan候補表を自前生成しない。候補生成をHALのeffective capabilityやdriver名で分岐せず、driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。

## サービス登録・publishability利用境界

`arib_si_engine_rs` が返すservice / transport単位の意味解析結果を、Android channel登録へ接続する判断はTISが所有する。

partial snapshot は サービス単位の登録可能判定に使ってよい。ただし partial snapshot を無条件に channel 登録へ出してはならない。global complete 判定だけで publish 可否を決めず、サービス / transport 単位の `publishability_by_service` と 登録可能判定で、service_id、TSID、ONID、PMT、PCR、必要 table、対応するaudioまたはvideo ESの欠落理由を分離する。登録可能サービスは、ONID / TSID / SID、PMT PID と PMT、有効 PCR、後続更新可能な internal key、および現行ライブ視聴で対応するaudioまたはvideo ESを持つサービスとする。video-onlyサービスは`TvContract.Channels.SERVICE_TYPE_AUDIO_VIDEO`、audio-onlyサービスは`SERVICE_TYPE_AUDIO`として登録可能にする。audio-onlyの視聴セッションでは`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`を通知できるが、この値をchannel登録の禁止理由に使わない。音声・映像の欠落または未対応はtrack別診断に残す。scrambled サービスは 登録可能 として channel 登録してよいが、現行の平文ライブ視聴成功対応宣言対象にはしない。登録可能未満の partial snapshot は 診断情報 / ライブ更新 / debugに限定し、channel insert に使わない。

## 録画・予約の現行除外

現行 product では録画・予約を製品機能として表明しない。TIS メタデータの `android:canRecord` は `false` のまま維持し、`MaleicacidTvInputService.onCreateRecordingSession()` は `null` を返す。`RecordingSession`、DVR/file output、`RecordedPrograms` 登録、`notifyRecordingStopped()` / `notifyError()`、`TvRecordingClient` による予約録画開始は現行 product 対象外である。

`rec/` 配下の実装とテストは録画・予約作業用の準備領域であり、現行 product package、TIS manifest、boot receiver、release確認条件へ混ぜない。現行 product で起動してよい receiver / サービスは TIS のライブ視聴・setup・EPG publish に必要なものだけとする。

## CAS / descrambler の現行境界

現行 product では CAS HAL 本体はプレースホルダーのままにする。TIS は Tuner SDK API の filter 経由で PMT/CAT/SDT/ECM/EMM section payload を取得し、PMT/CAT から得た CA_descriptor と SDT 等から得た free_CA_mode / サービス識別子 補助情報を arib_si_engine_rs / TIS 側で CA情報 / サービスメタデータ意味モデル に変換する。TIS はその CA情報 に基づいて ECM/EMM セクションフィルター と MediaCas/CAS bridge を型付き API で制御し、実 key トークン が得られた場合だけ Tuner descrambler へ不透明な参照値を渡す。仮実装 や診断専用結果は復号成功を意味しないため、`setKeyToken()` へ渡さない。Tuner HAL が未接続診断を返した場合も成功扱いにしない。

## Tuner SDK API 呼び出し

`openDescrambler()`、`setKeyToken()`、`addPid()`、`removePid()` は reflection を使わず、対象 build の system/privileged API として直接呼ぶ。API が利用できない build は現行 product 対象外とする。

## 再生経路

### Media3 version / Soong 境界

現行productのplaybackで使用するAndroidX Media3のbaseは **1.4.1** に固定し、対象LineageOS 21 / Android 14 treeの`prebuilts/misc`に偶然存在するMedia3版を暗黙利用しない。Media3 1.5.0は`compileSdk=35`へ移行すると同時に従来のmanual API-level outliningを削除し、non-Gradle buildではR8相当のautomatic API outliningを要求する。対象LineageOS 21 treeはAndroid 14世代のSoong / R8であるため、この追加toolchain前提を未証明のまま1.5.x AARへ持ち込まない。製品側ではGoogle Maven由来の`media3-common:1.4.1`、`media3-exoplayer:1.4.1`、`media3-extractor:1.4.1`とPOM dependency closureを同一version 1.4.1で固定する。

API 34のTV app AudioTrack attributionに必要な拡張点だけは、後続upstream 1.5.1の`DefaultAudioSink.AudioTrackProvider`、`DefaultAudioTrackProvider`、`DefaultAudioSink.Builder.setAudioTrackProvider()`に相当する変更を**1.4.1 `media3-exoplayer`へ限定backport**する。backportはAudioTrack生成factoryの差し替え境界だけに限定し、decoder、AudioSinkのwrite/clock、buffering、frame scheduling、track selectionその他のMedia3挙動を変更しない。upstream 1.4.1 artifact hash、backport patch hash、生成したproduct-local prebuilt hashを固定し、patch内容がこの境界以外へ広がった場合は別設計変更として扱う。

Soong導入はAOSP `prebuilts/misc/common/androidx-media3`と同じ`android_library_import` + `static_libs`方式を使う。製品root module名は`maleicacid_media3_common_1_4_1`、`maleicacid_media3_exoplayer_1_4_1_attribution`、`maleicacid_media3_extractor_1_4_1`に固定し、dependency closureにも`maleicacid_` prefixを付けてplatform moduleと衝突させない。TIS APKは必要なroot moduleを`static_libs`で明示参照する。platform側`androidx.media3.*` moduleへのfallback、異version混在、runtime download、Gradle解決、Media3 1.5.xを対象Android 14 R8で未検証のまま取り込むfallbackは行わない。

1.4.1には現行設計が必要とする`Player.Listener.onRenderedFirstFrame()`、`MimeTypes.APPLICATION_MEDIA3_CUES`を処理する`TextRenderer`、bitmap `Cue`、`CueEncoder`、`CUE_REPLACEMENT_BEHAVIOR_REPLACE`が存在する。したがってplayback clock / renderer / text timelineはupstream 1.4.1の標準ownershipに残し、製品差分はAudioTrack Builderへsession attributionを設定するfactory hookだけとする。

現行 product の平文 non-tunneled AV入力は、Tuner `MediaEvent.getLinearBlock()`をTISのMedia3 `MediaSource`／`SampleStream` adapterへ渡す経路を正式経路とする。`MediaEvent.isPtsPresent()` が true のsampleでは、raw PTSを90 kHz、33 bit modulo値として扱う。`PlaybackPipeline` は playback generation ごとに `PtsNormalizer` を1個だけ所有し、連続するraw PTSの差を modulo `2^33` の最短signed差としてunwrapしてgeneration内のextended PTSへ変換する。extended PTSから `presentationTimeUs = floor(pts90k * 1000000 / 90000)` をoverflow-safeなchecked integer arithmeticで算出し、Media3 `DecoderInputBuffer.timeUs`へ設定する。`isPtsPresent()` が false のsampleはAOSP Tuner API上で表現可能な入力状態として扱い、0、前sample、PCR、wallclock等から時刻を捏造せず、そのsampleだけをMedia3へ渡さず解放してtrack別`MISSING_PTS_SAMPLE`診断へ計上する。PTS欠落sample単体を理由にplayback generationを再生不能へ遷移させず、`notifyVideoUnavailable()`も呼ばない。first frame前ではPTS欠落sampleをMedia3入力可能状態への進捗として数えず、既存の`decoderStartupDeadlineMs`を延長またはリセットしない。first frame後も当該sampleだけを破棄し、generation全体のunavailable遷移は既存のplayer／decoder error、lock喪失、startup/backpressure deadline等の独立条件だけで判定する。このsample自体をTuner HALのmalformed eventとは扱わない。

`PtsNormalizer` の状態はretune、新playback generation、filter flush、player／decoder再生成、非wrap discontinuityで破棄する。通常の33 bit wrapだけは同generation内の連続差としてunwrapし、独自media clock、PCR→wallclock変換、独自future/late schedulerへ拡張しない。reflection、hidden API、ES全体の`ByteArray`中継、多重copyを禁止する。`SampleStream.readData()`で要求されたsampleだけを`LinearBlock.map()`でread-only参照し、有効rangeを`DecoderInputBuffer.data`へ1回copyする。`MediaEvent`、`LinearBlock`、input claimはcopy完了または破棄確定まで保持し、copy完了後に呼出側所有権を解放する。secure `MediaEvent`は現行平文productの対象外とし、mappable blockへの暗黙変換を行わない。

`getLinearBlock()`がnull、offset／lengthがblock範囲外、Media3入力adapterへ安全に渡せない場合は`PLAYBACK_INPUT_UNAVAILABLE`または入力不正の型付き診断へ落とす。成功を偽装せず、現generationのfilter、未消費event、Media3 player／MediaSource／SampleStream adapter、startup queue、budget claimを解放して`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。

デコード、A/V同期、video frame scheduling／drop、AudioTrack、Surface提示は現行productではAndroidX Media3 ExoPlayerへ一括して委ねる。TIS自身はMediaCodec、MediaSync、AudioTrack、独自media clock、独自future/late schedulerをplayback ownerとして持たない。`ExoPlayer.Builder(sessionContext)`でcurrent playback generation専用playerを生成し、TISはTuner AV filterから受けた圧縮sampleをMedia3 `MediaSource`／`SampleStream` adapterとして供給する。video outputは`player.setVideoSurface(currentSessionSurface)`でcurrent `sessionSurface`へ直接設定し、audio output、decoder選択、A/V clock、frame release scheduling、late dropはMedia3 renderer群へ所有させる。

Tuner `MediaEvent.getLinearBlock()`はTIS input adapterまでの所有物とする。Media3 `SampleStream.readData()`の公開契約は`DecoderInputBuffer.data`の`ByteBuffer`へsample dataを供給する形なので、現行productではadapterが`LinearBlock`の有効rangeをread-only mapし、そのrangeをcurrent `DecoderInputBuffer`へ1回copyしてPTSとflagsを設定する。copy完了後に該当`MediaEvent`／`LinearBlock`とTIS budget claimを解放する。Media3へ渡した後のcompressed input buffer、decoder output、audio buffer、render queueの寿命はMedia3が所有する。従来のTIS-owned `MediaCodec.CONFIGURE_FLAG_USE_BLOCK_MODEL`／`QueueRequest.setLinearBlock()` zero-copy経路は、このownership graphと公開Media3入力契約を同時には満たせないため現行productの正式playback経路から外す。header解析のためのmapとMedia3 adapterへの1回copy以外にES全体の多重copyやByteArray中継を追加しない。

first-frame availabilityのcommitはMedia3 `Player.Listener.onRenderedFirstFrame()`を使う。このcallbackはsurface設定、renderer reset、stream変更後にframeが初めてrenderされた時点のイベントなので、current playback generationとcurrent Surface generationへlistener tokenを結び付ける。`notifyVideoAvailable()`はこのcurrent tokenの`onRenderedFirstFrame()`を受け、current `sessionSurface`が有効、視聴制限でblockされておらず、同generationのplayer／video renderer errorがない場合だけ一度呼ぶ。Media3内部のframe-available、decoder output、clock進行、drop、旧generation／旧Surface callbackをfinal commitへ昇格させない。物理display/compositor fenceは要求せず、固定delay、独自clock、独自frame scheduler、hidden API、pixel probeも追加しない。

MediaSync案は完成経路として採用しない。`MediaCodec.OnFrameRenderedListener`は`MediaSync.createInputSurface()`へのdecoder出力到達しか観測せず、`MediaSync.getTimestamp()`はcurrent playback positionのtemporal gateには使えても、そのcandidate video frameがMediaSyncからoutput `sessionSurface`へrenderされたかlate-dropされたかを区別しない。AOSP MediaSyncはaudio同期時にlate video bufferをrenderせずinputへ返す経路を持つため、audio由来media positionがcandidate PTSを越えた後でもcandidateがdrop済みという実行が成立する。この状態で`notifyVideoAvailable()`を発火すると、`TvInputService.Session`の「content rendered onto its surfaceがready for viewingになったら通知する」という契約を満たす根拠がない。CTSが`notifyVideoAvailable()` callback伝播を検査するだけで実Surface timingを検査していないこと、Tuner VTSがTISのpresentation timingを検査しないことは、このAPI前提条件を緩和する契約ではない。よって`OnFrameRendered` + `getTimestamp()` best-effort gateは診断・中間観測には使用可能でもavailabilityの最終commitには採用せず、final rendererがrender/dropを区別して返す現行Media3 ownershipを維持する。

## EIT と TvProvider

現行releaseで収集するEIT table範囲、短期補完の用途、長期・他service・予約/追従利用のrelease境界は、tv直下の`開発規則.md`のr51到達点を唯一の正本とする。本書はそのscopeを再定義せず、TIS runtimeにおけるfilter起動・停止、Programs書き込み契機、retry、現在番組解決、視聴セッション利用だけを定義する。Programs の `internal_provider_data` には JSON v1 の stable `programKey`、timing、CAS state、長形式イベント項目、component/audio メタデータ、series 完全構造、イベントグループ `relatedItems`、linkage、free_CA_mode、音声言語、レーティング、診断 JSON を TIS 内部データとして保存する。TvProvider の標準 column には title / short description / long description、broadcast genre、明示写像できる canonical genre、series id、episode display number、scrambled、audio language、コンテンツレーティング など、`ARIB_SI_EPG_TvProvider投影方針.md` で自然対応が固定された範囲だけ反映する。last episode number は通常の `TvContract.Programs` 標準列へ投影しない。

TvProvider標準列への投影判断は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とする。`internal_provider_data` の schema、canonical encode、保存上限、診断 schema は `arib_si_engine_rs/DESIGN_JA.md` と Rust serde 構造体を正とする。本書は TIS runtime における取得、書き込み契機、retry、現在番組解決、視聴セッションでの利用だけを定義する。

### 複数table instance収集と停止

複数のtable instanceを包括的・継続的に取得する必要がある操作では、TISは`TableInfo repeat=true`を使用する。Tuner HALに未知の全instance集合の列挙や終端推測を要求しない。

TISは、現在の操作目的と`開発規則.md`のrelease scopeから、その操作で必要なinstance集合を決定する。`arib_si_engine_rs`が返すinstance別の完成・更新・寿命状態を用い、必要な集合が完成した時点でfilterを明示的に`stop()`する。

## 字幕表示の責務

ARIB 字幕は TIS 側の字幕 path で `libaribcaption` を使用する。現行 product では PMT から字幕 track を検出し、`TvTrackInfo.TYPE_SUBTITLE` として通知し、`onSetCaptionEnabled()` と字幕表示経路を接続する。字幕 track を advertise する場合は、ARIB 字幕 PES を libaribcaption C API 経路で処理し、実際に表示できることを対応宣言条件に含める。`arib_si_engine_rs` の自前 ARIB 文字列 decoder はサービス名・番組名・番組説明など字幕以外の SI/EPG 文字列に限定し、字幕 PES や字幕本文をその decoder に渡さない。libaribcaption は C API のみを使用し、独自 C/C++ 薄層 は書かない。Kotlin から直接 C API を呼ばず、TIS Kotlin → Rust JNI boundary → 安全なRustラッパー → libaribcaption C API の順に接続する。BML / data broadcast 実行環境、双方向データ放送 UI、データ放送 UI は恒久対象外である。

現行製品profileの字幕取得は、PMTで字幕ESを検出した場合だけ`TYPE_TS / SUBTYPE_PES`を開き、字幕PIDと明示`streamId=0xBD`（`private_stream_1`）で設定する。STD-B24 6.4-E1 Fascicle 1の9.1.1、9.2、9.3、9.5、9.6を独立PES字幕、data group、PTS、PMT descriptorの根拠とし、STD-B32 3.11-E1 Fascicle 3の3.1を`private_stream_1=0xBD`と宣言長付きPESの根拠とする。これはTIS字幕経路が選ぶ利用設定であり、Tuner HALのPES capabilityを`0xBD`へ制限する契約ではない。HAL正本は有効な明示`streamId 0..255`、wildcard `0xFFFF`、映像`0xE0..0xEF`の長さ0 PESを同じ広告済みPES能力で受理する。現行TISは字幕取得でwildcard、別stream ID、長さ0映像PESを要求しないが、それらをHAL非対応と推定または再定義してはならない。一般PESを利用するTIS機能を追加する場合は、同じ公開HAL契約をそのまま使用する。


## libaribcaption Soong / renderer 統合境界

ARIB字幕表示は、repoで供給される `libaribcaption-android` の product fork を Soong build graph に入れ、renderer 有効の `libaribcaption.so` として生成したものだけを正式経路とする。out-of-graph の `.so`、renderer 無効 build、`dlopen()` 確認だけ、decoder API 呼び出しだけ、Canvas 文字描画だけを 字幕対応宣言条件にしてはならない。

`libmaleicacid_arib_caption_jni` は `libaribcaption` に明示依存し、`MaleicacidTvInput` は JNI library として `libmaleicacid_arib_caption_jni` を取り込む。TIS は字幕 PES を Rust JNI boundary と安全な Rust ラッパー経由で libaribcaption C API に渡し、renderer 出力を字幕 overlay へ接続する。字幕 PES を受け取っても renderer 表示に到達できない状態を字幕対応成功として扱ってはならない。

### 字幕PTS scheduling / clear ownership

字幕のpresentation clockとdisplay/clear schedulingはvideo/audioと同じMedia3 ExoPlayer timelineが唯一所有する。TIS、Rust JNI、`CaptionOverlayView`は独自timer、`player.currentPosition` polling loop、`SystemClock`比較、MediaSync position追従loopを持たない。ARIB字幕PESから得たPTSはvideo/audioと同じ33-bit 90 kHz unwrap規則でcurrent playback generationの`timeUs`へ変換し、libaribcaption rendererへそのPTSで描画要求する。rendererが返すRGBA8888画像と描画領域はKotlin側で`Bitmap`へ安全に所有権変換し、Media3 1.4.1のbitmap `Cue`へ変換する。

字幕用Media3 `SampleStream`は`Format.sampleMimeType = MimeTypes.APPLICATION_MEDIA3_CUES`に加えて、**`Format.cueReplacementBehavior = CUE_REPLACEMENT_BEHAVIOR_REPLACE`を明示設定**する。defaultの`MERGE`には依存しない。各字幕eventのsampleは`DecoderInputBuffer.timeUs = subtitlePtsUs`とし、有限のlibaribcaption `wait_duration`は`CueEncoder.encode(cues, finiteDurationUs)`へそのまま渡す。`DURATION_INDEFINITE`は有限値へ推測変換せず、REPLACEでのみ許容される`C.TIME_UNSET`を`durationUs`としてencodeする。後続captionが到着した場合は、その後続PTSの新しいreplacement Cue sampleが前のindefinite Cueを置換するため、既にMedia3へ渡したsampleのdurationを遡って書き換えない。libaribcaption/ARIBの明示clearまたは当該PTSで表示すべき画像が空になるeventは、そのclear PTSに**空Cue listのreplacement sample**を`C.TIME_UNSET`で投入し、同じTextRenderer timeline上で消去する。

Media3 `TextRenderer`がplayerの`positionUs`に対してCueの開始・終了・replacementを評価し、current cue setを`TextOutput` / player cue callbackへ出す。`CaptionOverlayView`はそのcallbackで渡されたcurrent bitmap Cue群だけを合成し、空Cue群では即座に消去する。overlay自体は時刻判定を行わない。字幕track無効化、`onSelectTrack(TYPE_SUBTITLE, null)`、retune、Surface/session release、playback generation変更はPTS付きcaption eventではなくsession state transitionなので、subtitle SampleStreamを無効化すると同時にTextRenderer stateとoverlayを同期clearする。旧generationのlibaribcaption結果やCue callbackはgeneration tokenで破棄する。

bitmap Cueの`CueEncoder`経路はfreeではない。Media3 1.4.1の`Cue.toSerializableBundle()`はbitmapをlossless PNGへencodeしてParcel bytesへ格納し、`CueDecoder`側でPNGをBitmapへdecodeするため、RGBA8888 → Bitmap → PNG encode/Parcel → PNG decode → Bitmapというcopy/CPU負荷を持つ。この経路を字幕対応readyとする前に、製品profileの最大字幕plane/region更新頻度を使って、PNG encode/decode latency、peak allocation、GC、subtitle SampleStream backlogが既存のbounded memory/backpressure契約内に収まり、A/V playbackを阻害しないことを実機qualificationで確認する。未確認または上限超過を「たぶん軽い」として成功扱いにせず、そのprofileでは字幕対応を有効化しない。性能問題を独自timer、別clock、無制限queueで回避してはならない。

これにより既存future_workの「libaribcaption rendererのRGBA8888をKotlin overlayへ表示する」という完了条件を維持しつつ、表示・消去時刻のownerだけをMedia3 timelineへ固定する。future_workを独立timer実装の根拠として解釈してはならない。

## ライブ playback 実装方式

TIS のライブplaybackは、Tuner AV filterの`MediaEvent.LinearBlock`をTISのMedia3 1.4.1 `SampleStream` adapterで受け、必要rangeを`DecoderInputBuffer`へ1回copyして同じMedia3 1.4.1 ExoPlayerへ供給する経路に固定する。ExoPlayerがdecoder、audio sink、A/V clock、video scheduling／drop、text timeline、current `sessionSurface`への提示を所有し、TISはその外側にMediaCodec／MediaSyncや独自clock／frame scheduler／字幕timerを置かない。

`tunneled`／platform passthrough playback pathは現行productの設計候補から外し、実装しない。`notifyVideoAvailable()`は、current player/current Surface tokenに対するMedia3 `Player.Listener.onRenderedFirstFrame()`だけをvideo成功commitとして扱い、current Surface有効、generation一致、視聴制限、player／video renderer errorの各gateを満たした場合だけ一度通知する。frame available、decoder output、clock進行、drop eventをfinal commitへ昇格させない。

setup scan の channel registration は global discovery complete を必須条件にしない。ただし partial snapshot を無条件に channel insert に使ってはならない。TvProvider のサービス単位の登録可否は本書の「サービス登録・publishability利用境界」を唯一の正本とし、この節で video ES 必須などの追加 gate を重複定義しない。したがって `service_type=0x01` は同節の audio-video / video-only 条件、`service_type=0x02` は対応 audio ES を持つ audio-only 条件に従い、`0x02` の登録に video ES を要求しない。登録可能未満の partial snapshot は 診断情報 / ライブ更新 / debugにのみ使い、channel insert しない。scrambled サービスは channel 登録してよいが、CAS 仮実装 のまま 平文ライブ視聴成功 対応宣言 してはならない。

## codec header / A-V sync / publish mode の固定

ライブ playback の codec 構成は、現行 product では video は MPEG-2 video と H.264/AVC、audio は AAC と MPEG audio を対象 codec とする。現行 product が対象とする transport profile で追加 codec を扱う場合は、`開発規則.md` の ARIB 本文選定規則に従う条項根拠と、MediaFormat、decoder 起動、AudioTrack、first-frame gate、unsupported 診断情報の契約を設計正本へ固定してから扱う。STD-B79 / STD-B80 の高度地上方式が現行 product scope 外である間、それらの方式だけに追加された codec を現行 playback capabilityへ入れない。

現行製品が登録対象とするARIB `service_type`は、`0x01`のdigital television serviceと`0x02`のdigital radio sound serviceに固定する。`0x01`は`TvContract.Channels.SERVICE_TYPE_AUDIO_VIDEO`、`0x02`は`TvContract.Channels.SERVICE_TYPE_AUDIO`へ写像する。その他のservice typeは壊れたサービスへ丸めず、`UNSUPPORTED_SERVICE_TYPE`を記録して現行製品スコープ外としてchannel登録しない。対応集合を追加する場合は、ARIB上の意味、TvProvider写像、PMT成立条件、再生経路を同じ変更で追加する。

`service_type=0x02`は本来的なaudio-only serviceである。少なくとも1本の現行対応audio ESと物理選局情報、`ServiceKey`、inputId、表示名が揃えば、video ESを要求せず`SERVICE_TYPE_AUDIO`として登録し、audio filter・decoder・AudioTrackだけを開始する。視聴sessionでは映像filterを開かず、サービス分類確定後に`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)`を通知し、audio再生の成否と映像なし通知を分離する。audio codec非対応またはaudio ES欠落は`AUDIO_ONLY`の正常理由ではなく、`UNSUPPORTED_AUDIO_CODEC`または`SERVICE_TYPE_PMT_MISMATCH`として再生不能にする。

`service_type=0x01`はaudio-video serviceであり、現行対応video ESがない場合にaudio-onlyへ再分類しない。弱信号またはlock喪失は`VIDEO_UNAVAILABLE_REASON_WEAK_SIGNAL`、有効なserviceでdecoder起動またはqueue補充を一時待機する場合だけ`VIDEO_UNAVAILABLE_REASON_BUFFERING`、video codec非対応またはPMT構成不整合は`VIDEO_UNAVAILABLE_REASON_UNKNOWN`と型付き診断`UNSUPPORTED_VIDEO_CODEC`／`SERVICE_TYPE_PMT_MISMATCH`へ分離する。HEVCなど未対応codecのmetadataはprovider-dataへ保存してよいが、再生可能表明には使わない。

現行対応 video ES が存在し、audio ES が存在しない、または audio codec だけが現行未対応の場合は、video-only サービスとして視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。STD-B32 4.0以降の改定概要で高度地上デジタルテレビジョン放送向けに追加された MPEG-H 3D Audio / AC-4 は、STD-B79 / STD-B80 の高度地上方式が現行product scope外であるため現行codec固定表へ追加しない。AC-3 / Enhanced AC-3 も現行対象transportに対する条項根拠を確認せず推測で追加しない。

PMTからcodec family、audio/video種別、PIDを確定した後、AV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。snapshotは製品profileで事前検証した有限値として、`singleEventLimitBytes`、`startupQueueBudgetBytes`、`startupQueueMaxSamples`、`startupQueueMaxDurationUs`、`pendingQueueBudgetBytes`、`pendingQueueMaxSamples`、`pendingQueueMaxDurationUs`、`decoderStartupDeadlineMs`、`steadyBackpressureDeadlineMs`を持つ。codec headerをまだ受信していないこと、またはdecoderが未構成であることを理由に値を動的導出しない。全codec共通の8 MiB、4 sample、1000 msへ固定せず、対象codec、対象decoder/device組合せ、最大access unit、header収集量、reorder depth、allocator上限、実機最悪値からofflineで検証する。正の有限値と必要領域を開始前に予約できないprofileはAV filterを開始しない。

startup queueと台帳claimを確保した後にAV filterを開始し、上限内の`MediaEvent`からMPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio headerを収集してMedia3 `Format`へ写像する。header解析に必要な最小範囲だけ`LinearBlock.map()`でread-only参照してよいが、ES本体を`ByteArray`へ複製してはならない。必要なformat情報が成立したらMedia3 playerをprepareし、startup queueの`MediaEvent`／`LinearBlock`は`SampleStream.readData()`要求に応じて`DecoderInputBuffer.data`へ1回copyして解放する。decoder capability不足またはplayerのdecoder初期化失敗は`DECODER_CAPACITY_MISMATCH`または`UNSUPPORTED_*_CODEC`の型付き診断に落とし、filter、pending sample、claim、Media3 playerを回収して`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。

MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= LinearBlock capacity`を満たす場合だけstartup queueまたはMedia3 input adapterのpending queueへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。

必要なqueue領域とclaim台帳はplayback generation開始時に原子的に予約する。各eventはrange検証後、Media3 input adapterへenqueueする前に`dataLength`をsnapshot台帳へclaimし、いずれかのevent、byte、sample、duration上限を超える場合は原因別に`SAMPLE_TOO_LARGE`または`PENDING_QUEUE_FULL`を記録してHAL handleを解放する。claim済みbyte、sample、durationは`SampleStream.readData()`へのcopy完了、破棄、generation変更、stop、releaseで正確に返す。HALの`avPerFilterLiveBytes`または`avRuntimeBudgetBytes`をTISへ公開・複製・1event上限化しない。

first frame前はcodec-specificな`decoderStartupDeadlineMs`を用い、必要なsequence header、SPS/PPS、audio config、reorder用入力を収集している間の一時queue増加を通常backpressure失敗へ写像しない。startup deadlineまでにdecoder入力可能状態またはfirst frameへ到達できず、queueのbyteまたはduration上限も解消しない場合だけplaybackを停止して`notifyVideoUnavailable()`へ進む。first frame後は別の`steadyBackpressureDeadlineMs`を用い、単発超過は当該sampleを解放して継続し、期限中にdequeue進行がなくqueue上限が継続する場合だけunavailableへ遷移する。audioだけの超過はvideo-only継続可否を既存規則で判定し、無条件にvideo unavailableへ写像しない。

A/V同期方式とownership graphは現行productでMedia3 ExoPlayerに固定する。TISはTuner filter／Media3 input adapter／session lifecycleだけを所有し、ExoPlayerがdecoder、audio sink、playback clock、video renderer、frame scheduling／dropを所有する。`PlaybackPipeline`のserial executorはcurrent player generation、Surface generation、Tuner filter、input adapter、pending `MediaEvent`／`LinearBlock`、budget claim、player listener tokenを単一管理し、player callback／Tuner callback／parental callbackはstateを直接変更せず同executorへ直列化する。

input adapterはMedia3 `MediaSource`／`SampleStream`として実装し、Tuner sampleのPTS、codec format、EOSをMedia3へ公開する。`SampleStream.readData()`がsampleを要求した時だけpending `LinearBlock`の対象rangeをread-only mapして`DecoderInputBuffer.data`へ1回copyし、`timeUs`と必要flagsを設定する。copyが完了したsampleはTuner側ownershipとbudget claimを即時返却する。Media3が受理した後のbuffer lifetime、decoder input/output、audio queue、video frame queueはExoPlayer内部ownershipとし、TISはcodec output IDやAudioTrack bufferを保持しない。pending queue満杯時だけ既存budget規則で入力sampleをdropし、それ以外を無通知破棄しない。

video outputは`player.setVideoSurface(sessionSurface)`でcurrent Surfaceへ設定する。Surface設定または変更ごとにSurface generationを進め、`Player.Listener.onRenderedFirstFrame()`をcurrent player generation/current Surface generationへ関連付ける。Media3 Player契約上、このcallbackはsurface設定、renderer reset、stream変更後のfirst rendered frameを通知するため、これをTIF availability commitとして使用する。TISはMedia3内部のframe release時刻計算、late判定、drop判定を再実装しない。

audio outputは`ExoPlayer.Builder(sessionContext)`から生成したplayerの標準audio renderer／AudioSinkへ所有させる。session attributionはplayer生成Contextとして`sessionContext`を渡すことで同generationへ閉じる。TISが別途AudioTrackを生成してplayer外から同期させる経路は持たない。video-onlyではaudio trackを選択しない／audio rendererを無効化し、audio-onlyではvideo Surfaceを設定せず`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`契約を維持する。

retune、playback generation変更、stop、非wrap PTS discontinuity、decoder fatal、player fatalではcurrent player、MediaSource、SampleStream adapter、pending sampleをreleaseして新player generationを作る。Surface変更だけではplayer全体を必須再生成せず、旧Surfaceをclearして新Surfaceを設定しSurface generationを進める。旧player generationまたは旧Surface generationのlistener callbackは状態更新に使わない。通常33-bit PTS wrapはadapter内でunwrapし、wrapだけではgenerationを変更しない。

最低試験契約は、Tuner sample→Media3 SampleStream adapter、LinearBlock range検証、Media3入力への1回copy、PTS欠落sample単体dropとgeneration継続、通常PTS wrap、`onRenderedFirstFrame()`前はvideo availableにしないこと、current player/current Surface token以外のfirst-frame callbackを無視すること、drop／clock進行だけでは通知しないこと、Surface／parental／player-error gate成立後に一回だけavailability通知すること、retune／fatal後の旧player callback非採用、Surface切替後の旧Surface callback非採用、audio/video-only、player release時のpending Tuner ownership回収を含む。試験のqueue数値上限は選択した`ProductProfile`と一致させる。

TvProvider公開モードは `PublishMode` で channel row 追加を setup scan / explicit rescan に限定する。ライブ tune refresh、boot EPG sync、background channel maintenance では既存 channel の番組・診断更新だけを許可し、新規 channel row は追加しない。

## ARIB SI/EPG のTvProvider投影

ARIB SI/EPG の標準列投影は tv直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode は `arib_si_engine_rs` の Rust provider-data serde構造体を SSOT とする。TISは、同文書で標準列投影が固定された項目だけを TvProvider 標準列へ出し、標準列へ自然対応しない項目は JSON v1 `internal_provider_data` のみに構造化保存する。

`Programs.COLUMN_CANONICAL_GENRE` については、TIS が直接設定する値と、Android TvProvider が `Programs.COLUMN_BROADCAST_GENRE` から内部補完した読み出し結果を区別する。現行仕様では `ARIB_SI_EPG_TvProvider投影方針.md` の明示写像表に一致する分類だけを `ContentValues` に直接設定する。写像不能分類、reserved、extension、others、user_nibble 由来分類は直接設定しない。

`Programs.COLUMN_BROADCAST_GENRE` には、`arib_si_engine_rs` から受け取った ARIB content_descriptor の分類値とARIB表示名を、TIS が `TvContract.Programs.Genres.encode(...)` 形式で格納する。TIS は ARIB分類を Android canonical genre に推測変換しない。

## 視聴制限 / コンテンツレーティング 契約

TIS は `arib_si_engine_rs` から受け取った `parental_rating_descriptor` の構造化データを、AOSP system-defined ISDB レーティングドメイン（`com.android.tv / ISDB / ISDB_<age>`）の `TvContentRating` へ変換する。Android `TvContentRating` の domain / ratingSystem / レーティング 文字列は TIS 側で固定し、Rust 側のSSOTにしない。

TvProvider へ番組を登録または更新する場合、変換できる レーティングは `TvContentRating.flattenToString()` の結果を `Programs.COLUMN_CONTENT_RATING` に格納する。変換できない レーティングは推測で `COLUMN_CONTENT_RATING` に入れず、`internal_provider_data` と診断に保持する。

ライブセッション は、現在番組のレーティング と system 視聴制限 設定を同期して扱う。`TvInputManager.isParentalControlsEnabled()` が true の場合、TIS は現在番組の `TvContentRating`、または レーティング 未取得時の `TvContentRating.UNRATED` を `TvInputManager.isRatingBlocked(...)` に渡して判定する。blocked の場合は video frame を表示する前に再生を停止または抑止し、`notifyContentBlocked(rating)` を呼ぶ。許可された場合は `notifyContentAllowed()` を呼ぶ。

TIS は `TvInputManager.ACTION_BLOCKED_RATINGS_CHANGED` と `TvInputManager.ACTION_PARENTAL_CONTROLS_ENABLED_CHANGED` を監視し、設定変更時に現在番組の 視聴制限判定を即時再評価する。

## TIS/arib_si_engine_rs 固定事項

- LineageOS 21／Android 14の通常ライブセッション生成では`onCreateSession(inputId, sessionId, tvAppAttributionSource)`をoverrideする。framework由来`sessionId`は`Tuner(serviceContext, sessionId, useCase)`へ渡し、`tvAppAttributionSource`はsession固有Contextの生成へ渡す。2引数版`onCreateSession(inputId, sessionId)`と1引数版は明示的な互換経路だけに限定し、対象productの通常3引数入口を素のservice Contextへ委譲または後退させない。
- 現行の video 対応宣言対象は MPEG-2 video `0x02` と H.264/AVC `0x1b` に限定する。HEVC `0x24` は 現行平文ライブ視聴 / playback selection 対象外であり、診断上は `NO_SUPPORTED_VIDEO_ES` 相当として扱う。現行productが対象とするtransport profileで認識codecを追加する場合は、規範対象の現行ARIB原文と、実際に検証証拠として使用した取得可能本文の版・条項および未証明差分、codec固定表、playback selection境界を同じ設計変更で固定してから扱う。
- ARIB 視聴年齢制限 は Android `TvContentRating` へ `domain=com.android.tv`, `ratingSystem=ISDB`, `rating=ISDB_<age>` として写像する。対応範囲は JPN かつ レーティング 4..20 のみとし、未対応 country / レーティングは推測変換せず `internal_provider_data` / 診断に残す。レーティング 未取得時は `TvContentRating.UNRATED` として 視聴制限判定する。
- `notifyVideoAvailable()` はcurrent Media3 player/current Surface generationの`Player.Listener.onRenderedFirstFrame()`を受けた後だけ一度呼ぶ。clock進行、decoder output、drop、旧player／旧Surface callbackはavailability確定根拠にしない。物理display/compositor fenceは要求しないが、rendererより前段のcallbackで代用もしない。固定delay、独自clock、独自frame scheduler、hidden API、pixel probeは使わない。
- ライブ tune refresh では新規 channel row を作らず、既存 channel の program 更新だけを行う。setup/rescan のみ channel row を作成できる。
- H.264 は SPS/PPS 検出だけでなく SPS 由来の width / height を MediaFormat へ反映する。SPS 解析不能時は固定 1920x1080 代替処理 で成功扱いしない。
- PMT 由来の video/audio/subtitle track は `TvTrackInfo` として通知し、`onSelectTrack(TYPE_AUDIO, trackId)` と `onSetCaptionEnabled()` を受ける。現行 product では字幕 track と libaribcaption 表示経路を実装対象に含める。別 video track と data track 選択は、対応 codec / 実行環境がない限り 対応宣言しない。
- CS110 は stream selector `NONE` のみ許可し、TSID / relative selector を HAL tune request へ渡さない。Android Tuner builder では NONE 時に selector setter を呼ばない。
- boot 後 EPG 再同期は既存 channel の p/f 最小更新に限定し、新規 channel row は作成しない。`JapanIsdbScanPlan.defaultInitialScan()` は setup scan / explicit rescan 専用であり、boot EPG sync の既定候補に使わない。
- background channel maintenance は現行スコープ内の必須実装とする。ただし boot critical path から分離し、boot EPG sync 完了後または明示的保守タイミングで実行開始を試行する。実行開始は scan/maintenance が未実行で、かつ ライブセッション が存在しない場合に限る。active ライブセッション または scan 実行中の場合は開始せず、skip 理由を 診断情報に残す。対象は既存 channel と既存 transport メタデータ refresh までに限定し、新規 channel insert は行わない。
- セクションフィルター は CRC protected section で `setCrcEnabled(true)` を使用し、Rust 側 CRC 検査を defense-in-depth として維持する。TIS 側には PID / table / 状態 別 counter を持つ。


## 視聴年齢制限 / CAS 代替参照の固定

- `Programs.COLUMN_CONTENT_RATING` と Live session の 視聴制限判定は同じ `AribRatingMapper` を使い、`com.android.tv / ISDB / ISDB_4..20` の AOSP system-defined レーティングに統一する。
- TIS は custom レーティング-system XML / receiver を追加しない。product 統合時は システムTVアプリ / レーティング definitions に `com.android.tv / ISDB / ISDB_4..20` が存在することを確認する。
- Live session は 現在番組 レーティングを `TvProvider current Program -> latest EIT cache -> TvContentRating.UNRATED` の順で解決する。
- parental blocked の通知は `notifyContentBlocked(rating)` と AV停止を主とし、parental block の通知手段として `notifyVideoUnavailable()` を呼ばない。
- `onUnblockContent()` の解除範囲は同一 `channelUri + serviceKey + eventId + ratingString` の 現在番組 / レーティングに限定する。start/end は stable identity ではなく、解除対象が現在表示中の同一 Program row であることを確認する補助条件としてのみ使ってよい。start/end/duration を provider-data `programKey`、unblock stable identity、または Program identity の SSOT にしてはならない。
- CAS 未完成 / scrambled unsupported で 再生成功 にしない場合は `TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` を使う。具体的な CAS 状態 reason は CAS HAL 本実装まで使わない。
- Programs CAS 状態は current complete 診断 を優先し、不完全または欠落 診断 では既存 channel `internal_provider_data` の `requiresCas` / `unsupportedCas` / `clearLivePlaybackSupported` / `channelRegistrationReady` / `epgPublishable` を 代替処理 する。

## TIS / EPG 公開境界

現行の EIT publish/delete 対象は、TvProvider に channel が存在する `ServiceKey`、または同一 setup/rescan transaction で channel insert が成功して channelId が確定した `ServiceKey` に限定する。ライブセッション の `currentService` だけには限定しない。Program row を持たない サービス へ Programs を publish/delete してはならない。

現行r51の EIT publish/delete 対象 table は present/following actual `0x4E` のみとする。present/following other `0x4F`、schedule actual `0x50..0x5F`、schedule other `0x60..0x6F` は r51 の Programs publish/delete 対象外であり、更新区間を発生させない。r53以降で対象を拡張する場合は `開発規則.md` のrelease scopeを先に更新する。

EIT 更新時の update/削除区間 は、追加・変更・削除された event の既存 `[start,end)` と新 `[start,end)` の union とする。現行仕様では長期固定 lookahead window を導入しない。長期 EPG lookahead window を扱う場合は、EIT scope / version / event identity / authoritative 条件を設計正本へ固定してから併用する。EIT table scope の version 変更で既存 section が消えた場合は、消えた event の既存 window も 廃止行削除 対象に含める。

ただし、廃止行削除 の根拠にできる EIT section / table snapshot は Rust parser が `deletionAuthoritative=true` と判定したものに限る。start_time BCD、duration BCD、event descriptor_loop_length、event fixed フィールド が malformed の event を含む section は、既存 event 削除用の authoritative valid-event-set として扱わない。malformed event は既存正常 Program を消す根拠にせず、DescriptorDiagnosticV1 / ParserDiagnosticV1 に記録する。

Direct Boot保留の正式状態を`DirectBootEpgPending`とする。`BootEpgSyncCoordinator`がinputIdごとにdevice-protected storage上のこの状態を所有し、boot EPG sync要求を受理した時点または未完了・失敗終了時に設定する。状態はprocess restartとuser unlockをまたいで保持し、background maintenanceは設定・解除しない。

同一boot EPG sync taskがcancelされず、`collectSiForCandidate()`が`COMPLETE`となるcandidateを1件以上得て、登録対象channel／Programに必要なTvProvider必須問い合わせとinsert/update/deleteが一つのpublish transactionとして全て成功したcommit後にだけ`DirectBootEpgPending`を解除する。provider query/write failure、publish fingerprint生成失敗、cancel、登録可能サービス0件では保留を維持する。candidate成功だけ、部分write、またはfingerprint cache更新だけを解除根拠にしない。

登録可能サービスは、`ServiceKey`、物理選局情報、inputId へ戻せる channel provider data、表示名が揃い、TvProvider channel insert/update に進める サービスとする。表示名は `ChannelRecord.displayName` が nonblank ならそれを使い、なければ SDT service_name、さらに無ければ `service-<onid>-<tsid>-<sid>` を使う。この 代替処理名は 登録可能判定上の有効な 表示名と扱う。

## CAS 仮実装 境界

CAS HAL 仮実装 のまま scrambled サービスを 平文ライブ視聴 再生成功 として扱ってはならない。scrambled unsupported サービス でも、PMT/CAT/CA情報 と診断を使って EPG / Programs / レーティング / provider-data は更新する。ただし CAS key トークン を提供できない状態では 再生成功 にせず、CAS 起因の unavailable のみ `VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` へ map する。初回映像到達timeout、filter start failure、非対応stream、codec失敗、audio失敗 は CAS unknown に map しない。

CAS provider-data は current診断を優先する。`caStateResolved=true` で CAS required / unsupported / clearLivePlaybackSupported が判断できる場合は、`freeCaModeResolved=false` でも current診断を採用する。代替参照した診断情報を使った場合は `publishStateSource=fallback` とし、採用可能な状態がない場合は `publishStateSource=none` とする。

Descrambler API の `setKeyToken()`、`addPid()`、`removePid()` は戻り値が `Tuner.RESULT_SUCCESS` の場合だけ成功とする。非 SUCCESS result は CAS 診断 failure として扱い、成功扱いで握り潰してはならない。

## TvProvider failure semantics

TvProvider query failure と channel なしは別状態として扱う。既存 channel query が失敗した場合は `skippedNoChannel` として扱わず、failure 診断とし、publish fingerprint更新・`DirectBootEpgPending`解除 の根拠に使わない。

TvProvider query は 必須問い合わせ と 任意問い合わせ を区別する。チャンネル・番組の追加または更新、廃止行削除、既存チャンネル・番組検索、provider-data代替参照、Direct Boot準備完了 判定に使う query は 必須問い合わせ とする。必須問い合わせ で `ContentResolver.query()` が null cursor を返した場合は `TvProviderQueryFailure` とし、empty result とみなさない。`TvProviderQueryFailure` が発生した サービス/window では channel insert、program insert/update、廃止行削除、publish fingerprint cache更新、`DirectBootEpgPending`解除 に進まず、再試行区間 を保持する。

Programs publish/delete が provider failure になった場合は、`ProgramPublishCoordinator` の process-local retry queue に `ServiceKey + updateWindow + failureClass` を key として enqueue する。backoff は 1分、5分、15分、60分、以後最大60分、jitter ±20%、最大10回、保持期間24時間または次回正常 snapshot までとする。次回 `publishLiveProgramsForCurrentService()`、boot EPG sync、background maintenance の publish entrypoint 先頭で、`now >= earliestEligibleAtMillis`のentryだけを実行対象としてdrainする。未到達entryはqueueに保持し、entrypointが来ない限り指定時刻でのwake-upは行わない。成功した key は削除し、失敗した key はattemptを進めて新しい`earliestEligibleAtMillis`を設定する。process restart では retry queue を破棄し、boot/background sync による再収集を正とする。provider failure 時は 廃止行削除、publish fingerprint更新、`DirectBootEpgPending`解除 に進まない。

retry queue は全体上限 512 windows、ServiceKey ごと上限 32 windows とする。超過時は古い順に破棄し、ServiceKey 別 `droppedRetryWindowCount` を加算する。process restart 後は counter を 0 に戻す。

SDT-other / NIT-other / BAT 由来で現在 candidate の actual transport に解決できない サービスは、現在 candidate の物理情報で channel insert しない。未登録で Program row が存在しない unresolved transport は scan/maintenance 診断情報に `skippedUnresolvedTransportCount` として記録し、Program provider-data には書かない。publish 済み Program には自 サービスの `skippedUnresolvedTransport=false` を入れる。

## provider-data 利用境界 / publish fingerprint

`Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` の具体 schema、正規化、安定キー抽出、保存上限は `arib_si_engine_rs/DESIGN_JA.md` の「provider-data / diagnostics Rust SSOT」と `arib_si_engine_rs/schema/*.schema.json` を正とする。TIS は保存 schema を再定義しない。

TIS Kotlin は provider-data JSON を `JSONObject.put()` や文字列連結で直接構築してはならない。TIS Kotlin は Rust JNI の build / 正規化 / key extraction API で得たbytesをTvProviderに書く。TIS が JNI へ渡す JSON は Rust builder への入力 DTO であり、TvProvider に保存する provider-data schema ではない。

Program provider-data の top-level envelope、必須フィールド、検証規則、正規化、安定キー抽出は TIS では再定義しない。正本は `arib_si_engine_rs/DESIGN_JA.md`、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json`、`arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json` とする。TIS instrumentation テスト用の期待値 JSON を置く場合は Rust 側テストデータとバイト単位で同一に保つ。

TIS は `components.video[]`、`components.audio[]`、`components.subtitle[]`、`components.data[]` を provider-data schema として再定義しない。TIS は TvProvider 標準列、`TvTrackInfo`、MediaFormat / AudioTrack / 字幕表示経路へ接続する接着層に限定する。`audio` / `video` が `null` の場合は主 track 未選択または未確定を意味し、空オブジェクトと同義に扱ってはならない。

Program publish fingerprintは、同一process内で同じ公開transactionをTvProviderへ重複書き込みしないためだけに使用する。TvProviderへ実際に書く`ContentValues`（provider-data bytesを含む）と更新windowを、固定column list順の`<columnName>\0<byteLength>\0<bytes>`へ直列化し、そのSHA-256 lowercase hexをprocess-local cacheにだけ保持する。TvProvider rowやprovider-dataには保存せず、診断、真正性、改ざん検出、永続identityには使用しない。insert後にprovider-dataを再生成した場合は、実際に書いた最終bytesからfingerprintを再生成する。この行全体fingerprintがprovider-data bytesの同一性も包含するため、provider-data単体のdigestは生成しない。

publish fingerprint は、provider-data bytesを含む TvProvider へ実際に書く最終 `ContentValues` と更新windowだけを固定column順で直列化して計算し、JSON key単位の除外規則を設けない。TvProvider row id に依存する診断値を provider-data へ混ぜないことで、row作成後の診断更新が fingerprint を自己参照的に変更する構造を禁止する。


## 現在番組 選択

現在番組 resolver は TvProvider query 時点で `START_TIME_UTC_MILLIS <= now AND END_TIME_UTC_MILLIS > now` に絞る。sort order は `START_TIME_UTC_MILLIS DESC, END_TIME_UTC_MILLIS ASC, _ID DESC` に固定する。overlap がある場合も cursor 返却順には依存せず、この selection rule で1件を選ぶ。

現在番組選択の診断は process-local `CurrentProgramResolutionDiagnostic` とし、`selectionRule`、`overlapCount`、`selectedProgramId` を保持できる。`selectionRule` は `START_DESC_END_ASC_ID_DESC` とし、対象なしの場合は empty string とする。この診断は `Programs.COLUMN_INTERNAL_PROVIDER_DATA` へ永続化せず、publish fingerprint、Program identity、unblock identity の構成要素にしない。ARIB `event_id` は `COLUMN_EVENT_ID` と JSON v1 `programKey.eventId` で扱う。

## CA descriptor / provider-data 直列化

CA_descriptor の raw bytes は Rust parser が元 section から保持し、JNI snapshot DTO に raw bytes として渡す。Kotlin 本番経路 code で CA_descriptor を再構築しない。malformed CA_descriptor は 元記述子 / CAS メタデータ から除外し、サービス自体は保持する。診断情報には `malformedCaDescriptorCount` と table/PID/サービス context を残す。Kotlin 側で修復して provider-data や CASメタデータに不正な元記述子を入れてはならない。

malformed CA_descriptor の詳細診断は、CAS 検出 snapshot またはサービス / channel provider-data 診断を一次保存先とする。Program provider-data には、その Program 公開時点で参照した service / CAS 診断の summary として `malformedCaDescriptorCount` を保存してよい。ただし raw descriptor、table/PID/サービス context の完全情報を Program ごとに重複展開してはならない。Program 側 summary は CAS メタデータや再生可否判定の根拠ではなく、公開時点の診断参照結果として扱う。

## transaction DTO / provider-data SSOT / executor / setup / retry の固定

### Rust JNI provider-data API

TIS Kotlin は provider-data JSON を解釈せず、以下の Rust JNI API 相当だけを使う。

```kotlin
object NativeProviderData {
    external fun buildProgramProviderData(inputJson: String): ProviderDataResult
    external fun normalizeProgramProviderData(rawBytes: ByteArray): ProviderDataResult
    external fun extractProgramKey(rawBytes: ByteArray): ProgramKeyResult?
    external fun buildChannelProviderData(inputJson: String): ProviderDataResult
    external fun decodeChannelProviderData(rawBytes: ByteArray): ChannelProviderDataResult?
}

data class ProviderDataResult(
    val bytes: ByteArray,
    val schemaVersion: Int,
    val truncated: Boolean,
    val diagnosticsDroppedCount: Int,
)

data class ChannelProviderDataResult(
    val canonicalBytes: ByteArray,
    val schemaVersion: Int,
    val serviceKey: ServiceKey,
    val tune: ChannelTune,
)
```

`ChannelTune` は `inputId`、`deliverySystem`、`frequencyHz`、`streamIdType`、`streamId`、`physicalChannel`、`satelliteBand`、`remoteControlKeyId` だけを持つ typed tune 復元値とし、backend名、driver名、px4相対slot等のbackend固有値を持たない。`decodeChannelProviderData()` は invalid UTF-8、malformed JSON、schema不整合を null または診断付き失敗へ落とし、Kotlin側でJSONを解釈・修復しない。

`inputJson` は Rust builder への入力 DTO であり、TvProvider に保存する provider-data schema ではない。最終JSONバイト列、正規化、安定キー抽出はRustが行う。provider-data単体のdigestまたはsignatureは返さない。

`rawBytes` は任意バイナリではなく、既存 TvProvider に保存済みの JSON v1 UTF-8 バイト列を指す。Kotlin は `String(rawBytes)` などで再解釈してから Rust へ渡してはならず、TvProvider から取得した `COLUMN_INTERNAL_PROVIDER_DATA` の BLOB バイト列をそのまま Rust JNI 境界へ渡す。TvProvider が文字列として返した場合の互換補助は、UTF-8 バイト列へ戻すだけに限定し、Kotlin 側で JSON 構造を解釈・再構築してはならない。

`normalizeProgramProviderData(rawBytes)`、`extractProgramKey(rawBytes)`、`decodeChannelProviderData(rawBytes)`は、invalid UTF-8またはmalformed JSONをKotlin側で修復しない。Rustは診断付き失敗、key抽出失敗、またはchannel decode失敗へ落とし、通常実行経路で例外やpanicに変換しない。provider-data bytesだけのdigest APIと`ProviderDataResult.signature` / `contentDigest`は設けない。

### 診断情報 schema

Descriptor診断 の機械検証規則は `arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json` を正とする。TIS は `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` 配下のオブジェクトを別 schema へ変換せず、Rust JNI が返した provider-data JSON 内の診断情報を保存する。未対応の視聴年齢制限は `ratings[]` に構造化値を残し、補足説明が必要な場合だけ `diagnostics.publishDiagnostics[]` に warning を追加する。TIS Kotlin は descriptor diagnostic JSON を独自生成しない。

### provider-data 保存上限

provider-data の soft limit / hard limit、診断情報・長文補助情報の切り詰め規則、切り詰め時の診断 key は `arib_si_engine_rs/DESIGN_JA.md` と Rust provider-data 実装を正とする。TISは保存前にRust JNIが返したbytesをそのまま扱い、Kotlin側で独自の切り詰めschemaを定義しない。


### SectionEvent 入力上限

TIS の PSI/SI section path は allocation 前に `SectionEvent.dataLength` を検証する。`MAX_SECTION_BYTES` は 4096 bytes とし、`dataLength` が 1..4096 の範囲にある場合だけ ByteArray 確保と `AribSiEngine` / CAS / Program publish への投入を許可する。section read size 不一致、0 length、負値相当、4096 bytes 超過は parser に渡さず 診断カウンター に記録する。

### transaction DTO API

`AribSiEngine` 呼び出し側 は複数 snapshot を合成してはならない。本番経路 は以下の用途別 bulk DTO を使う。

```kotlin
data class ProgramPublishSnapshot(
    val snapshotGeneration: Long,
    val ingestSequence: Long,
    val events: List<AribEvent>,
    val updateWindows: List<EpgUpdateWindow>,
    val publishabilityByServiceKey: Map<ServiceKey, ProgramPublishability>,
    val descriptorDiagnostics: List<DescriptorDiagnostic>,
    val parserDiagnostics: List<ParserDiagnostic>,
    // CAS 診断一次保存先から Program provider-data summary へ渡す ServiceKey 別件数。
    // Program ごとに raw descriptor / table / PID context を重複展開しない。
    val malformedCaDescriptorCountByServiceKey: Map<ServiceKey, Int>,
)

fun takeProgramPublishSnapshot(): ProgramPublishSnapshot
```

```kotlin
data class ServiceRegistrationSnapshot(
    val snapshotGeneration: Long,
    val services: List<AribService>,
    val actualTransports: Set<TransportKey>,
    val publishabilityByServiceKey: Map<ServiceKey, ProgramPublishability>,
    val diagnostics: List<ParserDiagnostic>,
)

fun serviceRegistrationSnapshot(): ServiceRegistrationSnapshot
```

```kotlin
data class CasDiscoverySnapshot(
    val snapshotGeneration: Long,
    val services: List<AribService>,
    val caMetadata: List<CaMetadata>,
    val pmtPids: Map<ServiceKey, Int>,
    val catEmmPids: List<Int>,
    val diagnostics: List<DescriptorDiagnostic>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic>,
)

fun casDiscoverySnapshot(): CasDiscoverySnapshot
```

`MalformedCaDescriptorDiagnostic` は、少なくとも `pid`、`tableId`、`tableIdExtension`、`serviceId`、`elementaryPid`、`scope`、`offset`、`declaredLength`、`actualRemainingLength`、`reason`、`rawPrefixHex` を持つ。詳細診断の一次保存先は CAS discovery snapshot とし、Program provider-data は `malformedCaDescriptorCount` summary だけを保存する。

`takeProgramPublishSnapshot()` は events / updateWindows / publishability / 診断情報を同一ロック / 同一 native state から取得し、updateWindows の drain もこの API 内だけで行う。`snapshotEvents()` と `takeEpgUpdateWindows()` を 本番経路 呼び出し側 で別々に呼ぶことは禁止する。LiveSession の現在番組判定、視聴年齢制限判定、映像メタデータ補完のように updateWindows を消費してはならない read-only 参照は `programStateSnapshot()` を使い、drain 型 state を返してはならない。

廃止 snapshot wrapper は本番経路・公開通常境界・product build に残してはならない。テスト専用に必要な入口は test source または test-only 可視性に隔離し、本番 APK / JNI API / release API から参照不能にする。

### LiveSession / PlaybackPipeline / Scan の直列化

`MaleicacidLiveSession` は session-level serial executor を持ち、current サービス、generation、playback 署名、track state、unblock state、latest video メタデータ、`ProgramPublishCoordinator` へのアクセスを同一 executor に閉じる。TunerController、PlaybackPipeline、parental receiver の コールバック は直接 state mutation せず、session executor に enqueue する。

`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。Tuner filter、Media3 MediaSource／SampleStream adapter、ExoPlayer、player generation、Surface generation、pending Tuner sample、budget claim、listener tokenの変更を呼び出し元スレッドで直接行わない。release後のqueued taskはreleased flagとgenerationで破棄する。

`ChannelScanManager` は scan generation と purpose を持つ。cancel / cleanup task は対象 generation にだけ作用し、stale cleanup が後続 scan の `running`、controller、engine を変更してはならない。

### SetupActivity 保護

`SetupActivity.onCreate()` は scan を自動開始しない。scan 開始前に正規 setup flow の inputId が自 TIS の inputId と一致することを検証する。inputId 欠落または不一致時に 代替処理 inputId で scan へ進まない。scan は検証済みユーザー操作または検証済み setup request の後に開始する。

product 側で システムTVアプリ に grant 可能な場合、SetupActivity は 署名 / privileged permission で保護する。permission grant が成立しない target でも、自動 scan 禁止、inputId 検証、ユーザー操作開始は必須とする。

SetupActivity は自分が開始した `SETUP_SCAN` purpose かつ同一 scan generation の Completed だけで `RESULT_OK` にする。過去の Completed、boot EPG sync、background maintenance の Completed で finish してはならない。

### Direct Boot drain / ライブセッション 優先

`MaleicacidTvInputService.onCreate()` は Direct Boot pending drain、boot EPG sync、background maintenance を開始しない。Boot EPG sync / background maintenance は BootReceiver、UserUnlockReceiver、または明示的な maintenance scheduler からのみ起動する。

Boot EPG sync / background maintenance の開始条件は、`activeLiveSessionCount == 0`、`sessionCreationInProgress == false`、`setupScanRunning == false`、`playbackPipelineRunning == false`、`scanManager running == false` をすべて満たすこととする。ライブセッション 作成要求が来た時点で boot/background task が未開始なら defer する。boot/background task が既に running の場合、現行仕様では boot/background task を cancel/defer し ライブ tune を優先する。


## TIS コールバック 入力境界と逆圧

- `SectionEvent.dataLength` は、Tuner コールバック から読み取る section の正確な byte 長として扱う。
- TIS が section event として受け付ける長さは `1..4096` byte だけとする。`dataLength <= 0` は不正、`dataLength > 4096` は過大として、どちらも `ByteArray` 確保前に破棄し、PID 別診断に計上する。
- `MediaEvent` sampleは固定4 MiBを上限にしない。負のoffset、0以下のlength、加算overflow、`offset + length > LinearBlock capacity`は不正入力として確保前に破棄する。正常sampleは同一製品profileのper-event予算をclaimしてMedia3 input adapterのpending queueへ渡し、`SampleStream.readData()`時に有効rangeだけを`DecoderInputBuffer.data`へ1回copyする。共有領域方式とイベント固有fd方式を同じpending byte予算へ計上する。
- Tuner→Media3 input adapterの逆圧は無通知破棄ではない。未読`MediaEvent`／`LinearBlock`は上限付きpending queueとbudget claimに保持し、Media3 `SampleStream.readData()`で消費する。sampleを破棄するのは上限付きqueueが満杯の場合だけとし、破棄counterを加算する。

## provider-data / retry / attribution 境界契約

### Provider-data SSOT

`TvContract.Channels/Programs.COLUMN_INTERNAL_PROVIDER_DATA` の新規書き込みは `arib_si_engine_rs` の provider-data JNI API が返す JSON v1 bytes をそのまま保存する。TIS Kotlin は TvProvider 標準列を詰める接着層であり、provider-data 本体、program stable key、descriptor 診断情報 schema、provider-data digestまたは署名を独自JSON schema として再構築してはならない。

Channel provider-data の新規書き込み・読み取り正形式は JSON v1 のみとする。`key=value;...` 形式、旧 flat provider-data、旧 provider-data 断片は読み取り互換入力としても残さない。JSON v1 は `schema="maleicacid.tv.channel"` / `schemaVersion=1` を持ち、channel tune 復元に必要な inputId、物理選局情報、ONID / TSID / service_id、表示名、登録可能性診断を Rust provider-data API 由来の構造として保存する。



### 旧 indexed JNI / 廃止経路の禁止

TIS は `nativeSnapshotBulkJson()` と provider-data JNI API を通常境界とする。`nativeGetEventCount()`、`nativeGetEvent*` indexed JNI getter、旧 event JSON `canonicalGenres` フィールド、互換専用の空返却シンボル、未使用 private external 宣言は残してはならない。旧経路を使う呼び出し不能コードや test-only 以外の廃止予定 path は、互換維持ではなく削除する。

### Program publish retry

Program publish retry queue は現行仕様では process-local とする。process death 後の retry 永続化は行わず、boot/background scan による再収集を正とする。ただし、process-local queue であっても retry key は `ServiceKey + updateWindow + failureClass`、entry は `attempt / earliestEligibleAtMillis / firstFailureAtMillis / lastFailureAtMillis` を持つ。1/5/15/60分 backoffと決定的jitter ±20%は、次回実行可能になる最短時刻`earliestEligibleAtMillis`の算出にだけ使い、その時刻でのwake-upまたは実行開始を保証しない。最大10回、24時間 retention を適用する。

Provider 必須問い合わせ failure、Program insert/update failure、廃止行削除 failure、publish fingerprint build failureではpublish fingerprint cache更新と `DirectBootEpgPending`解除 に進まない。廃止行削除 は `deletionAuthoritative=true` の 更新区間 でのみ実行する。

### AttributionSource

LineageOS 21の通常経路では、`TvInputService.onCreateSession(inputId, sessionId, tvAppAttributionSource)`で受け取ったnon-null `tvAppAttributionSource`をsession寿命中のattribution正本とする。session生成時に`serviceContext.createContext(new ContextParams.Builder().setNextAttributionSource(tvAppAttributionSource).build())`で変更不能なsession固有`sessionContext`を作り、`sessionId`、`tvAppAttributionSource`、`sessionContext`を同じsession creation snapshotへ確定する。途中失敗ではSessionを公開せず、作成済みartifactを解放する。

Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。audio出力はMedia3が所有するが、Android 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`によるTV app attribution chainを失ってはならない。現行productでは上記の**1.4.1 + upstream AudioTrackProvider限定backport**に含まれる`DefaultAudioTrackProvider`を継承したsession固有providerを使い、backportしたprotected hook `customizeAudioTrackBuilder(AudioTrack.Builder)`だけをoverrideして`setContext(sessionContext)`を追加する。`DefaultRenderersFactory.buildAudioSink(...)`のoverrideでは`DefaultAudioSink.Builder(sessionContext).setAudioTrackProvider(sessionProvider).build()`を返す。sample rate、channel config、encoding、buffer size、audio attributes、audio session id、offload等のAudioTrack構成はbackport元upstream `DefaultAudioTrackProvider`の標準実装に残し、TIS側へ複製しない。TISはAudioTrackへのwrite、playback head、clock、buffer schedulingを所有しない。Media3 1.5.x本体や1.9系の別provider APIには依存しない。通常経路で素の`serviceContext`へ後退せず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。session releaseまたはplayer置換後は旧`sessionContext`、旧player、旧AudioSinkを新generationへ再利用しない。

`setAttributionSource()`を探索・呼出しするreflection、hidden API、vendor独自AIDL、reflection失敗時の無言fallbackを通常経路に置かない。対象system APIを使う必要が生じた場合は、対象SDKへ直接コンパイルできる型付き呼出しとして別途設計する。


## 本プロダクト対象TSであり得るcodec固定表

ARIB 資料上の本プロダクト対象TSであり得る codec を追加認識対象にする場合は、次の固定表を設計正本に吸収してから扱う。ここでの「扱う」は、PMT / component descriptor / 音声コンポーネントdescriptor / stream type / codecメタデータを認識し、TvProvider / trackメタデータ / 診断情報へ正しく反映することを含む。ISDB-S3 / MMT / TLVは恒久対象外であり、それらだけに依存するcodecまたは音響構成を本表の根拠へ持ち込まない。

### 参照した ARIB 資料と根拠

ARIB適合性の規範対象と検証証拠の分離は `../開発規則.md` を正とする。STD-B32 の規範対象は製品scopeに適用される現行日本語原文4.1であり、レビュー環境の入手可否では変えない。現時点で条項単位に取得して検証証拠として使用できる公式本文は英語版3.11-E1であるため、下表の従来TS profile条項確認には3.11-E1を用いる。ただし4.1日本語原文を本レビュー環境で取得していないため、3.11-E1から4.1までの当該条項差分は未証明であり、下表だけをもって4.1への完全適合確認済みとは扱わない。ARIB公式の4.0/4.1改定概要は、高度地上デジタルテレビジョン放送向けにVVC、MPEG-H 3D Audio、AC-4等が追加・更新されたという適用範囲確認には用いるが、未取得4.x本文の具体条項を推測する根拠にはしない。現行r51〜r53はSTD-B79のISDB-T2/ISDB-T1.5およびSTD-B80のISDB-T3を対応宣言しないため、これら高度地上方式向け追加codecを現行capabilityへ自動追加しない。

| 根拠資料 | 本改訂で固定する内容 |
|---|---|
| ARIB STD-B32 3.11-E1 Fascicle 1 Chapter 3 3.1〜3.3 | 現行 product が対象とする従来TS profileについて、MPEG-2 Video、MPEG-4 AVC、HEVC の認識根拠として用いる。 |
| ARIB STD-B32 3.11-E1 Fascicle 2 Chapter 3 3.1〜3.4、Chapter 5、Chapter 6 | 現行 product が対象とする従来TS profileについて、MPEG-2 AAC、MPEG-2 BC、MPEG-4 AAC、MPEG-4 ALS の認識根拠として用いる。 |
| ARIB STD-B10 5.13-E1 Part 2 Table 6-5 / 6.2.26 / Annex E | 現行 product が対象とするTS signalingについて、MPEG-2 系映像、H.264/AVC、H.265/HEVC、MPEG-2 Audio、AAC ADTS、MPEG-4 Audio LATM の認識根拠として用いる。 |

### video codec

| codec | 追加認識時の扱い |
|---|---|
| MPEG-2 Video | 必須対応。PMT / component descriptor から codec、解像度、走査方式、aspect を認識し、Media3 Format写像、decoder capability確認、`onRenderedFirstFrame()` gate、unsupported 診断情報を固定する。 |
| H.264 / MPEG-4 AVC | 必須対応。profile / level は AVC video descriptor と実 MediaCodec capability を照合し、未対応時は codec unsupported 診断に落とす。 |
| H.265 / HEVC | codec として認識対象。対象 transport profile を本プロダクトが 対応宣言しない場合は ライブ viewable capability に入れない。対応する場合は Media3 Format写像 / decoder capability確認 / `onRenderedFirstFrame()` gate まで必須。 |

ISO/IEC 14496-2 Visual、JPEG 2000、auxiliary video、SVC、MVC、3D additional view は、今回の ISDB-T/S product scope の ライブ viewable codec として 対応宣言しない。必要なら provider-data / 診断情報に保持する。

### audio codec

| codec | 追加認識時の扱い |
|---|---|
| MPEG-2 AAC | 必須対応。ADTS / MPEG-2 AAC LC、channel count、sample rate、ISO639 language、main/sub、dual mono、音声モード、音質表示を保持する。 |
| MPEG-2 BC Audio | 認識対象。decoder が利用できる場合だけ再生対応を 対応宣言 し、未対応時は video-only 診断に落とす。 |
| MPEG-4 AAC / HE-AAC | 必須認識。AAC LC / HE-AAC profile、LATM/LOAS / ADTS、AudioSpecificConfig、channel count、sample rate を保持する。decoder が利用できる場合だけ再生対応を 対応宣言する。 |
| MPEG-4 ALS | codec として認識対象。対象 transport profile を本プロダクトが 対応宣言しない場合は playable capability に入れない。対応する場合は Media3 Format写像 / decoder capability確認 / audio renderer／AudioSink / メタデータ / unsupported 診断情報 まで必須。 |

MPEG-H 3D Audio と AC-4 は ARIB STD-B32 4.0以降の改定概要で高度地上デジタルテレビジョン放送向け追加codecであることを確認できるが、STD-B79 / STD-B80 の高度地上方式を現行productが対応宣言しないため現行codec固定表には含めない。AC-3、Enhanced AC-3、DTS、DTS-HD、Dolby TrueHDも現行対象transportに対する取得可能なARIB本文の条項根拠を確認せず推測で追加しない。

## provider-data 受け渡し境界（推奨案A）

TIS は TvProvider 標準列への投影を担当する。TIS は `Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` に保存される最終 JSON を直接生成してはならない。

TIS が JNI へ渡す JSON は、保存形式ではなく Rust へ値を渡すための受け渡し用形式である。この受け渡し用形式の型、必須項目、欠落時の扱い、旧形式拒否、値域検査は Rust の serde 型を正とする。TIS はこの受け渡し用 JSON を provider-data schema の Kotlin 実装、保存形式または正規形として扱ってはならない。

受け渡し用形式の schema 名は `maleicacid.tv.programRequest` / `maleicacid.tv.channelRequest` とし、保存用 schema 名 `maleicacid.tv.program` / `maleicacid.tv.channel` を名乗らない。

Rust は受け渡し用 JSON を serde 型へ読み込み、検査し、保存用JSON、識別子、切り詰め診断を生成する。TIS は Rustが返した保存用JSONをそのままTvProviderの`internal_provider_data`に保存する。TISはRustが返した識別子と診断結果だけを使う。

TIS は保存データの型、正規化、必須項目判定、欠落補完、旧形式互換、識別子抽出、サイズ上限処理を実装してはならない。TIS 側で `0`、`false`、`jpn`、`UNKNOWN`、空文字などを使って必須項目欠落を補い、provider-data を成立させてはならない。

`DescriptorDiagnosticV1` は Rust が生成した正規 JSON を正とする。TIS は `DescriptorDiagnosticV1` を項目ごとに再構築してはならない。TIS が保持する場合は、Rust 生成の正規 JSON を不透明な文字列として透過保持する。

TIS の試験は、受け渡し用 JSON の細部を保存形式として検査しない。検査対象は Rust provider-data builder が返した保存用JSON、識別子、拒否診断に寄せる。
