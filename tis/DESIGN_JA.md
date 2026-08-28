# TIS 設計判断

## AOSP 標準経路

TIS は `TvInputService` としてシステムTVアプリから呼ばれ、Tuner HAL には Tuner SDK API 経由でアクセスする。HAL binder を直接呼ばない。
TIS の setup / boot EPG sync / user unlock drain は、固定文字列や package 名を inputId とみなしてはならない。`TvInputManager.tvInputList` から自 `MaleicacidTvInputService` に一致する `TvInputInfo.id` を一意に解決し、その inputId だけを scan / sync / TvProvider writer へ渡す。解決不能または複数一致の場合、boot EPG sync は pending のまま延期し、setup scan は開始しない。

## BS と CS110 の選局契約

BSはIF周波数とAOSP Tuner公開契約のtyped stream selectorを保持する。通常のscan候補、channel保存、再選局ではbackend種別に依存せず、`STREAM_ID`のTSID `0..65534`だけを使用する。TISはpx4の相対slot、Linux DVBの`DTV_STREAM_ID`、HAL内部のbackend capabilityを取得・推測・保存しない。CS110は周波数帯だけでscan candidateとtune selectorを作り、stream selectorを保存しない。

CS110のTIS内部モデルとTvProvider保存形式では、frontend stream selectorを`None`／`null`として保持する。Android 14 Tuner API builderへ変換するときは`streamId`と`streamIdType`のsetterをどちらも呼ばない。builderが生成する`STREAM_ID`と`INVALID_STREAM_ID(0xFFFF)`の組を、Tuner HALが公開契約境界で`NoSelector`へ正規化する。TISから`UNDEFINED`、0、TSID、relative番号を「selectorなし」の代用として明示設定しない。CS110 の ONID / TSID / service_id は channel identity / サービス識別子として保持してよいが、HAL frontend selectorへ転用してはならない。BSの通常製品経路はIF周波数と`STREAM_ID`のTSIDを使う。TISはdriver固有slotへ変換せず、typed selectorの検証とbackend ABIへの写像はTuner HALへ委ねる。

TvProvider の channel internal provider data には JSON v1 `tune.streamIdType` と `tune.streamId` を保存する。通常製品経路で書き込む値は、`NONE` の `streamId=null`、または `TSID` の `0..65534`だけとする。`65535`はAOSP `INVALID_STREAM_ID`であり、実TSIDとして保存または再投入しない。`RELATIVE`はAOSP Tuner AIDLで合法なtune-time selector種別だが、本製品では永続channel tune identityとして採用しないため、TISの通常channelデータへ保存しない。


## 製品 scan 候補表の保持者

製品scanの選局対象、周波数帯、CATV中心周波数、VHF除外、BS/CS110 selector境界を含む規範値は、tv直下の`開発規則.md`の「製品 scan 候補の規範値」を唯一の設計正本とする。

TISの候補表は製品scan実装データのSSOTであり、`開発規則.md`の規範値に従うscan候補の実装データを唯一保持する。実行時にexplicit tune candidateを生成し、Tuner HALへ渡すscan値はTISが生成したexplicit tune candidateに限定する。TIS以外の文書や実装に同等の候補表を重複保持せず、Tuner HALは日本向けscan候補表を自前生成しない。候補生成をHALのeffective capabilityやdriver名で分岐せず、driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。

## サービス登録・公開・再生policy境界

`arib_si_engine_rs` が返すservice / transport単位の `ServiceSemanticFacts` をAndroid channel登録、EPG公開、ライブ再生へ接続する判断はTISが所有する。`ServiceSemanticFacts` はONID / TSID / SID、ARIB `service_type`、PMT/PCRの存在・構文状態、ES/component一覧とcodec signaling、CA descriptor / free_CA_mode、CA descriptor等から導出した`requiresCas`、SMD意味状態、欠落・不正理由など放送由来の事実だけを含む。`channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackSupported`、`unsupportedCas`のような現在の製品能力・TIF policy結果は含まない。

TISはcurrent `ServiceSemanticFacts`から`requiresCas`を意味事実として受け取り、現在releaseの対応service type/codec、実decoder availability、CAS実装状態、TvProvider transaction条件と組み合わせて、serviceごとに `channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackSupported`、`unsupportedCas` を算出する。このpolicy結果はTIS runtimeの一貫した判断材料であり、SI parserへ逆流させず、保存済みprovider-dataをcurrent policyのfallback sourceにしない。

SMDの通常受信対象判定では、`arib_si_engine_rs`が返す`broadcasting_flag` / `broadcasting_identifier`の放送由来事実と、TISが現在処理している`ScanCandidate.kind`を組み合わせる。`ISDB_T_UHF` / `ISDB_T_CATV`は地上デジタルテレビ`0b000011`、`ISDB_S_BS`はBSデジタル`0b000010`、`ISDB_S_110CS`は広帯域CSデジタル`0b000100`を期待値とする。`broadcasting_flag=0b00`の正常SMDでもcandidateと`broadcasting_identifier`が一致しないserviceは、そのcandidateでは`channelRegistrationReady` / `epgPublishable` / `clearLivePlaybackSupported`にしない。ONIDから放送方式またはSMD期待値を推定せず、ONIDはnetwork/service identityとしてのみ用いる。

partial snapshot はサービス単位の登録可能判定に使ってよい。ただし partial snapshot を無条件に channel 登録へ出してはならない。登録可能サービスは、ONID / TSID / SID、PMT PID と PMT、有効 PCR、後続更新可能な internal key、および現行ライブ視聴で対応するaudioまたはvideo ESを持つサービスとする。video-only / audio-onlyというtrack構成は`TvContract.Channels.COLUMN_SERVICE_TYPE`の再分類根拠にせず、同列は`../ARIB_SI_EPG_TvProvider投影方針.md`に従ってARIB `service_type`のcodingを保持する。audio-onlyの視聴セッションでは`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`を通知できるが、この値をchannel登録の禁止理由に使わない。音声・映像の欠落または未対応はTIS側のtrack別診断に残す。scrambled サービスはTIS policyでchannel登録してよいが、現行の平文ライブ視聴成功対応宣言対象にはしない。登録可能未満の partial snapshot は診断情報 / ライブ更新 / debugに限定し、channel insert に使わない。

`TvTrackInfo` の `trackId` はAndroid/TIS runtimeの識別子であり、TISがcurrent serviceのcomponent identityからcurrent session内で一意になるよう決定する。ARIB意味objectや永続`internal_provider_data`に`trackId`を保存せず、Rust SI parserへ返さない。

## 録画・予約の現行除外

現行 product では録画・予約を製品機能として表明しない。TIS メタデータの `android:canRecord` は `false` のまま維持し、`MaleicacidTvInputService.onCreateRecordingSession()` は `null` を返す。`RecordingSession`、DVR/file output、`RecordedPrograms` 登録、`notifyRecordingStopped()` / `notifyError()`、`TvRecordingClient` による予約録画開始は現行 product 対象外である。

`rec/` 配下の実装とテストは録画・予約作業用の準備領域であり、現行 product package、TIS manifest、boot receiver、release確認条件へ混ぜない。現行 product で起動してよい receiver / サービスは TIS のライブ視聴・setup・EPG publish に必要なものだけとする。

## CAS / descrambler の現行境界

現行 product では CAS HAL 本体はプレースホルダーのままにする。TIS は Tuner SDK API の filter 経由で PMT/CAT/SDT/ECM/EMM section payload を取得し、PMT/CAT から得た CA_descriptor と SDT 等から得た free_CA_mode / サービス識別子補助情報を arib_si_engine_rs の意味解析結果として受け取る。TIS はcurrent `ServiceSemanticFacts`とcurrent CAS capabilityに基づいて ECM/EMM セクションフィルターと MediaCas/CAS bridgeを型付きAPIで制御し、実keyトークンが得られた場合だけTuner descramblerへ不透明な参照値を渡す。仮実装や診断専用結果は復号成功を意味しないため、`setKeyToken()`へ渡さない。Tuner HALが未接続診断を返した場合も成功扱いにしない。

## Tuner SDK API 呼び出し

`openDescrambler()`、`setKeyToken()`、`addPid()`、`removePid()` は reflection を使わず、対象 build の system/privileged API として直接呼ぶ。本製品buildはこれらのAPIを提供するplatformと一体で構成することを恒久的なintegration prerequisiteとし、欠くbuildを本製品構成として成立させない。runtimeでreflection、代替API、HAL binder直呼びへfallbackしてこの前提を回避しない。

## 再生経路

製品ライブAVのアーキテクチャは、`../開発規則.md` のproduct-level invariantどおり **clear-memory / non-passthrough** のAOSP Tuner標準経路に固定する。TISはTuner media filterが返す各`MediaEvent`について`getLinearBlock()`と`getOffset()` / `getDataLength()`で示された有効rangeを、そのまま`MediaCodec.CONFIGURE_FLAG_USE_BLOCK_MODEL`の`QueueRequest.setLinearBlock()`へ渡す。AOSP Tuner Frameworkが同じnon-passthrough media filterについてデータ形式をESまたはpartial ESとしつつ`MediaEvent.getLinearBlock()`を直接MediaCodecへqueueする利用フローを規定しているため、TISは`MediaEvent`境界の上にcodec別access-unit parser、AU再構成、PES再解析を追加しない。

TISはMPEG-2 Video / H.264 / HEVC / AAC / MPEG audioのstart code、NAL、slice、ADTS frame、MPEG audio frameを通常入力経路で再解析してqueue境界を作らない。`MediaEvent`がpartial ESを含み得ることはTuner→MediaCodec境界の契約として受け入れ、`BUFFER_FLAG_PARTIAL_FRAME`の手動付与、TIS所有`LinearBlock`への再構成、ES全体またはAU単位のcopyを標準経路にしない。対象decoder/device profileがこのAOSP direct-input契約を満たさない場合は、その組合せをplayback capability qualificationで非対応にする。runtimeでcodec parser/reassemblerへfallbackしてAOSPの責務境界を複製しない。

各`MediaEvent`のtimestamp metadataもevent単位で透過的に扱う。`MediaEvent.getOffset()`をPTSの適用位置またはPES header位置とは解釈しない。AOSP契約上、`isPtsPresent()`は元PES headerに明示PTSが存在したかというprovenanceを表し、`getPts()`はaudio/video frameの90 kHz presentation timestampを表す別フィールドである。本製品が成功対応として表明するclear / non-passthrough live media-filter profileでは、Tuner HAL / media-filter producerがすべてのnon-empty `MediaEvent`について、当該eventのESデータへ適用可能な有効な33-bit 90 kHz presentation timestampを`getPts()`で提供することをproducer/consumer契約とする。明示PTSを持つPES由来eventでは`isPtsPresent()==true`かつ`getPts()`はその明示PTSを表す。PTSを明示しない合法なPES由来eventでは`isPtsPresent()==false`を維持し、hardware demux / driver / backend media extractor等のproducer側が当該eventのESデータに対応するpresentation timestampをauthoritative timing metadataとして既に確定できる場合に限り、その値を`getPts()`へ設定する。HAL共通層は定数0、単純な直前PTS carry-forward、PCR、wallclock、nominal frame rate、sample rate等からpresentation timestampを推測生成しない。producer側境界でも当該eventとのauthoritative associationを確定できないbackend/profileは、このlive direct-input成功対応profileとして表明しない。`isPtsPresent`をtimestamp validity flagへ読み替えず、provenanceを偽装して`true`へ丸めない。

TISは`isPtsPresent()`をMediaCodecへqueueする／しない、drop、playback fatalの判定に使用しない。non-empty `MediaEvent`ではproducer-authoritativeな`getPts()`を33-bit range検証し、`PtsNormalizer`へ渡して同じeventの`QueueRequest.setPresentationTimeUs()`へ必ず設定してからdirect queueする。Android 14の`MediaCodec.QueueRequest`はpresentation timestampのabsenceを表現できずsetter未呼出しでは0がqueueされるため、setter未呼出しを「timestampなし」として利用しない。TISは0、直前PTS、PCR、wallclock、frame rate、sample rateからtimestampを補完せず、別eventやcodec AUへPTSを再関連付けせず、codec別AU parser、PES再解析、AU再構成も追加しない。producerが上記保証を満たせないbackend/profileはlive direct-input成功対応profileとしてqualificationを通さず、成功capabilityとして表明しない。これは公開Tuner AIDL/VINTF/VTSのフィールド意味を変更するものではなく、既存`MediaEvent.pts`を使って製品内producer/consumer責務を閉じる追加契約である。

最低試験は、(1) explicit PTS PESでは`isPtsPresent()==true`かつ`getPts()`がそのPTSになること、(2) 合法なPTS-sparse inputで`isPtsPresent()==false`でもbackendが当該media outputに対応するauthoritative timing metadataを持つ場合はその対応値を`getPts()`へ出し、TISがdrop/fatalせずqueue継続すること、(3) authoritative sourceがない場合にproducer共通層／TISのどちらも0、直前PTS、PCR、wallclock、frame rate、sample rate等からtimestampを推測生成せず、そのbackend/profileをlive direct-input成功capabilityとして表明しないこと、(4) 33-bit wrap前後とA/V間で本来のtimeline差を維持すること、(5) TISが`isPtsPresent()==false`だけを理由にdrop/fatalしないこと、を含める。

`PlaybackPipeline` はplayback generationごとに1個の`PtsEpochCoordinator`と、active compressed trackごとの`PtsNormalizer`を持つ。これはcodec framingやAU identityを所有せず、**producer contractで有効な`getPts()`を持つ全non-empty `MediaEvent`のtimestamp変換だけ**を担当する。`PtsNormalizer`の`rawPrev` / `extendedPrev`はtrack別とし、33-bit wrap epochだけをgeneration内で共有する。`M = 2^33`、`H = 2^32`、`signedDelta(rawNew, rawRef) = ((rawNew - rawRef + H) mod M) - H`を`[-H, H-1]`の差とし、半周期差は`-H`に固定する。generationで最初のproducer-authoritative `getPts()`を共通extended seed `H`へ置き、後から開始または置換されるtrackはそのtrackで最初のproducer-authoritative `getPts()`をcurrent coordinator referenceに対してsigned-moduloで同じepochへjoinする。seed済みtrackはtrack-localにunwrapする。`isPtsPresent()==false`でも`getPts()`は通常どおりcoordinator / normalizerへ入力し、provenance bit自体はunwrap状態やqueue可否を変更しない。PTS deltaの大小、通常wrap、presentation-order reorderだけから独自discontinuityを推定しない。

plain `Filter.flush()`はAOSP契約どおりfilterが生成済みで未消費のdataをclearする入力側操作とし、未queue `MediaEvent` / `LinearBlock`と対応claimだけを破棄する。plain flushだけでは`PtsEpochCoordinator`、seed済み`PtsNormalizer`、decoder、MediaSync、AudioTrack、playback generationをresetしない。decoder再生成、AudioTrack切替 / 再生成、audio route変更、Surface変更、またはcodec / PID / track graph変更を伴うretune・track切替は後段のlifecycle契約どおりfull playback generation resetとし、coordinatorと全normalizerを新seedから開始する。filterのstop/reconfigure/restartに伴う`RestartEvent`は旧configuration eventを捨てるevent-validity境界であり、playback graphが変わらない限りそれ単独ではgeneration resetにせず、任意のPTS jump検出器としても使わない。

`MediaEvent.getOffset()` / `getDataLength()`は`long`のまま境界検査し、`offsetLong >= 0`、`dataLengthLong > 0`、`offsetLong <= Int.MAX_VALUE`、`dataLengthLong <= Int.MAX_VALUE`を先に満たすことを要求する。次に`offsetLong + dataLengthLong`をchecked additionで算出して`long` overflowを拒否し、`LinearBlock`のqueue可能range / capacity以下であることを確認する。すべて満たした後だけ`Math.toIntExact()`相当でoffset / sizeを`int`へnarrowし、`QueueRequest.setLinearBlock()`へ渡す。implicit cast、truncate、narrow後のoverflow検査は禁止する。`getLinearBlock()`がnull、block model configureまたはQueueRequestが利用不能、range / narrowing検査違反、decoderがAOSP direct-input契約を満たさない場合は成功を偽装せず型付き診断へ落とし、当該playback profileを成功対応として表明しない。

source `MediaEvent` / `LinearBlock`とrangeに対応するbudget claimは、`QueueRequest.queue()`が成功してcodecへ所有権を移すか、当該eventの破棄が確定するまで保持する。TISは通常ES payloadを`LinearBlock.map()`から`ByteArray`、別`LinearBlock`、通常ByteBuffer input modelへ複製しない。header / MediaFormat確定のために必要な最小prefixをread-only参照することは許すが、それをAU parserまたはES搬送経路へ拡張しない。

secure-memory handle、tunneled playback、platform passthroughは本製品が提供しない恒久的なplayback capabilityであり、時点依存の一時除外ではない。TISはsecure `MediaEvent`をclear-memoryへcopyできると仮定せず、その経路をadvertiseしない。r52のCAS対応もdescramble後のclear ESをこのdirect non-passthrough経路へ接続できるサービスだけをライブ視聴成功対象とする。

デコード後のA/V同期とSurface提示はAndroid標準`MediaSync`だけを使用する。decoder output の `BufferInfo.presentationTimeUs` をMediaSyncへ渡すmedia timeの正本とする。video decoder出力は`MediaSync.setSurface(sessionSurface)`後の`createInputSurface()`へ接続し、output timestampをMediaSyncへ渡す。audio decoder出力PCMは`MediaSync.queueAudio()`へpresentation time付きで渡し、`MediaSync.Callback.onAudioBufferConsumed()`を受けるまでaudio output bufferの所有権を保持する。独自media clock、PCR→wallclock変換、独自future render / late drop schedulerを設けない。

### MediaSync Framework-private final-output observation

stock Android 14 / LineageOS 21 の `MediaSync` はvideo scheduling/dropをnative側で所有し、late frameをinputへ返すdrop分岐と、render対象frameをcurrent outputへattachして`queueBuffer()`する分岐を区別する。一方、公開Java APIにはそのfinal-output成功をvideo clientへ通知するcallbackがない。この不足だけを閉じるため、対象LineageOS platformの`android.media.MediaSync`へ、既存public `MediaSync.Callback`とは別の `@hide OnFirstVideoFrameQueuedToOutputListener` と、arm識別子を同時に設定する `@hide setOnFirstVideoFrameQueuedToOutputListener(long armSequence, listener, handler)` 相当を追加する。Framework側は`armSequence`をTIF/TIS固有の意味を解釈しないopaque値として保持し、listener eventは少なくとも`MediaSync` instanceと成功判定時に固定した`armSequence`を返す。public SDK、`@SystemApi`、`@TestApi`、Tuner AIDL/VINTFは変更しない。

availabilityをarmするたびに、`PlaybackPipeline`はimmutableな`AvailabilityArm`を作り、そのMediaSync instance専用の正の64-bit `armSequence`を1から単調増加で割り当てる。同じMediaSync instanceの寿命中は過去の`armSequence`を再利用しない。別MediaSync instanceではinstance identityとplayback generationが別の失効境界になるため、arm sequenceのglobal/session namespace、乱数nonce、live/retired token集合、collision checkは設けない。native MediaSyncはcurrent armの`armSequence`を保持するだけでavailability semanticsを解釈しない。arm中にvideo bufferがlate-drop分岐を通過し、current `mOutput`へのattachと`queueBuffer()`がともに成功した時点で、その**成功を判定したarmの`armSequence`をevent payloadへ固定してから**armを解除する。late-drop、attach失敗、queue失敗、output abandonment、inputへ返したbufferではeventを生成せず、arm状態も消費しない。re-arm後に旧eventのJava配送が遅延しても、event payloadのsequenceは生成時の旧armから書き換えない。64-bit sequence exhaustion／wrapを実運用上の回復経路として設計せず、これを理由に暗号乱数、永続retired集合、MediaSync再生成などの追加runtime recoveryを設けない。常時すべてのrender成功を通知せず、TISが必要な`AvailabilityArm`だけをarm/re-armする。

TISは**各accepted `onTune(Uri)`ごと**に新しいvideo availability obligationを生成する。videoを持つtuneではcurrent waiting armを取消してfresh `AvailabilityArm`を割り当て、各accepted `onTune(Uri)`ではcurrent waiting armを取消し、TIS playback graphを終了して新しいplayback generationと新MediaSync instanceのinitial `AvailabilityArm`を作る。TISはchannel同一性、frontend lock状態、pipeline healthを根拠に`Tuner.tune(settings)`の発行またはplayback generation再生成を省略しない。同一の正規化settingsで既存物理lockを安全に継続できる場合にbackend retuneを省略する判断は、frontend stateを所有するTuner HALだけが行う。当該tune受理後の新generationで生成されたcurrent final-output成功eventだけが、そのtuneの`notifyVideoAvailable()` obligationを満たす。audio-only serviceは既存契約どおり映像availabilityを偽装せず`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)`で閉じる。callback eventはMediaSync instance、playback generation、event payloadの`armSequence`を同executor上で照合し、sequenceがcurrent `waitingAvailabilityArm.armSequence`と一致し、current `sessionSurface`が有効、視聴制限でblockされておらず、同generationの`MEDIASYNC_ERROR_SURFACE_FAIL`がない場合だけ`notifyVideoAvailable()`を呼ぶ。受理またはarm取消し後はcurrent waiting armを無効化する。一度availableになった後、`VIDEO_UNAVAILABLE_REASON_BUFFERING`等のrecoverable unavailableへ遷移し**同じMediaSync instance/generationを維持して復旧する場合**もfresh armでre-armし、re-arm前の旧armで既に生成済みだった遅延eventはarm sequence不一致で必ず破棄する。fresh arm後のfinal-output成功eventだけで再びavailableへ遷移する。generation teardownを伴うunavailableは新MediaSync instanceのinitial armで閉じる。これによりper-`onTune()` obligationは常に新playback generationで閉じる。`available -> recoverable unavailable -> available`で同じMediaSync instance/generationを維持するre-armは、同一accepted tune内の一時的復旧だけに限定する。

callbackは物理display/compositorへのpresent fence完了を意味せず、video scheduling/drop ownerであるMediaSyncがrender対象を選択しcurrent final outputへのqueueを成功させたことだけをcommitする。`MediaCodec.OnFrameRenderedListener`、`MediaSync.getTimestamp()`、playback clock進行はvideo availability commitへ使用しない。native内部mutexを保持したままJavaへreentrant callせず、成功時に固定した`armSequence`を保持したままJNI/Java handlerへ非同期配送する。release済み、旧generation、旧MediaSync instance、またはTISのcurrent waiting armとarm sequenceが一致しない遅延eventはstate更新に使わない。

このcallbackは同一製品buildでFrameworkと同時更新されるplatform-private contractである。TIS APKは`/system_ext`のplatform-coupled componentとして同一platform sourceに対して型付きcompileし、reflection、hidden API allowlist回避、callback不存在時のtimestamp推測fallbackを置かない。Framework patchを持たないbuildは現行product playback contractを満たさないためintegration/build時に拒否する。

## EIT と TvProvider

現行releaseで収集するEIT table範囲、短期補完の用途、長期・他service・予約/追従利用のrelease境界は、tv直下の`開発規則.md`のr51到達点を唯一の正本とする。本書はそのscopeを再定義せず、TIS runtimeにおけるfilter起動・停止、Programs書き込み契機、retry、現在番組解決、視聴セッション利用だけを定義する。Programs の `internal_provider_data` には JSON v1 の stable `programKey`、start+durationのtiming、放送由来CAS意味事実、長形式イベント項目、component/audioメタデータ、series完全構造、`eventGroups`、linkage、free_CA_mode、ARIBレーティングraw値、診断JSONをTIS内部データとして保存する。音声言語は`components.audio[].language/secondLanguage`にだけ保持し、top-levelへ複製しない。runtimeで選択したaudio/video track、`TvTrackInfo.trackId`、Android canonical genre投影結果、Android rating文字列、decoder/CAS product capability、channel/EPG/live可否はprovider-dataへ保存しない。TvProvider の標準columnには title / short description / long description、broadcast genre、明示写像できる canonical genre、series id、episode display number、scrambled、audio language、コンテンツレーティングなど、`ARIB_SI_EPG_TvProvider投影方針.md`で自然対応が固定された範囲だけ反映する。last episode number は通常の `TvContract.Programs` 標準列へ投影しない。

TvProvider標準列への投影判断は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とする。`internal_provider_data` の schema、canonical encode、保存上限、parser/descriptor診断schemaは `arib_si_engine_rs/DESIGN_JA.md` と Rust serde 構造体を正とする。本書はTIS runtimeにおける取得、policy算出、書き込み契機、retry、現在番組解決、視聴セッションでの利用だけを定義する。

### 複数table instance収集と停止

複数のtable instanceを包括的・継続的に取得する必要がある操作では、TISは`TableInfo repeat=true`を使用する。Tuner HALに未知の全instance集合の列挙や終端推測を要求しない。

TISは、現在の操作目的と`開発規則.md`のrelease scopeから、その操作で必要なinstance集合を決定する。`arib_si_engine_rs`が返すinstance別の完成・更新・寿命状態を用い、必要な集合が完成した時点でfilterを明示的に`stop()`する。

## 字幕表示の責務

ARIB 字幕は TIS 側の字幕 path で `libaribcaption` を使用する。現行 product では PMT から字幕 track を検出し、`TvTrackInfo.TYPE_SUBTITLE` として通知し、`onSetCaptionEnabled()` と字幕表示経路を接続する。字幕 track を advertise する場合は、ARIB 字幕 PES を libaribcaption C API 経路で処理し、実際に表示できることを対応宣言条件に含める。`arib_si_engine_rs` の自前 ARIB 文字列 decoder はサービス名・番組名・番組説明など字幕以外の SI/EPG 文字列に限定し、字幕 PES や字幕本文をその decoder に渡さない。libaribcaption は C API のみを使用し、独自 C/C++ 薄層は書かない。Kotlin から直接 C API を呼ばず、TIS Kotlin → Rust JNI boundary → 安全なRustラッパー → libaribcaption C API の順に接続する。BML / data broadcast 実行環境、双方向データ放送 UI、データ放送 UI は恒久対象外である。

現行製品profileの字幕取得は、PMTで字幕ESを検出した場合だけ`TYPE_TS / SUBTYPE_PES`を開き、字幕PIDと明示`streamId=0xBD`（`private_stream_1`）で設定する。STD-B24 6.4-E1 Fascicle 1の9.1.1、9.2、9.3、9.5、9.6を独立PES字幕、data group、PTS、PMT descriptorの根拠とし、STD-B32 3.11-E1 Fascicle 3の3.1を`private_stream_1=0xBD`と宣言長付きPESの根拠とする。これはTIS字幕経路が選ぶ利用設定であり、Tuner HALのPES capabilityを`0xBD`へ制限する契約ではない。HAL正本は有効な明示`streamId 0..255`、wildcard `0xFFFF`、映像`0xE0..0xEF`の長さ0 PESを同じ広告済みPES能力で受理する。現行TISは字幕取得でwildcard、別stream ID、長さ0映像PESを要求しないが、それらをHAL非対応と推定または再定義してはならない。一般PESを利用するTIS機能を追加する場合は、同じ公開HAL契約をそのまま使用する。


## libaribcaption renderer runtime 契約

ARIB字幕表示は、repoで供給される `libaribcaption-android` の製品forkをSoong build graphに入れ、renderer有効の `libaribcaption` moduleを正式経路として使用する。build/link/partitionの統合条件は `INTEGRATION.md` を正とし、本書はTIS内部runtime、renderer viewport、PTS、native lifecycleだけを所有する。out-of-graph prebuilt、renderer無効build、`dlopen()`確認だけ、decoder API呼び出しだけ、Canvas文字描画だけを字幕対応宣言条件にしてはならない。

`libmaleicacid_arib_caption_jni` は Rust JNI boundary + 安全なRustラッパーから libaribcaption C APIを直接利用し、TISは字幕PESをdecoderからrendererへ渡してRGBA8888 renderer出力を字幕overlayへ接続する。字幕PESを受け取ってもrenderer表示に到達できない状態を字幕対応成功として扱ってはならない。renderer結果は、PTS、duration、image metadata、RGBAを含む長さ検査済みのone-shot packed result 1個としてRust-owned bufferからKotlin/Bitmap所有へ渡す。frame handle registry、imageごとのJNI往復、永続serializationは設けない。Kotlinへlibaribcaptionのraw pointerや借用寿命を漏らさず、caption/result/imageのcleanupはRust FFI境界で完結させる。

Rust JNIの表示用出力は文字列ではなくrenderer結果を表し、次のmodelを保持する。

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

libaribcaption所有bitmapはRust-owned `Vec<u8>`へcopyしてからcleanupし、JNIを越えた後はKotlin/Bitmap側が所有する。非同期UI queueへlibaribcaptionの借用bufferを露出しない。`CaptionOverlayView`はRGBAのchannel順を明示的にAndroid ARGB pixelへ変換してBitmap化し、後述viewport originを一度だけ加算して`drawBitmap()`する。`caption.text` / `Canvas.drawText()`を字幕表示正式経路に残さない。strideを無視して `width * 4` の密な配列と仮定せず、Bitmap生成前にwidth/height/stride/buffer sizeの整合を検査する。

### renderer viewport / 座標契約

libaribcaption rendererはrender前に `aribcc_renderer_set_frame_size()` を必ず成功させる。固定1920x1080、字幕plane size、端末display sizeを代替値として推測使用してはならない。

TISはplayback generationごとに `CaptionViewport` を一つ所有する。`CaptionViewport` はcurrent session Surfaceに対応して実際に字幕を重ねるvideo content viewportをoverlay座標系で表し、次を一組として持つ。

```text
CaptionViewport:
  overlayWidthPx   > 0
  overlayHeightPx  > 0
  contentLeftPx
  contentTopPx
  contentWidthPx   > 0
  contentHeightPx  > 0
```

`contentLeftPx/contentTopPx/contentWidthPx/contentHeightPx` はletterbox / pillarboxを含むoverlay全体ではなくcurrent video content表示矩形を表す。videoを持たないaudio-only serviceでは映像座標のrenderer viewportを成立させず、その経路で字幕表示成功を表明しない。

renderer frame sizeは `contentWidthPx x contentHeightPx` に設定する。libaribcaptionが返す `dst_x/dst_y/width/height` はrenderer frame左上を原点とする座標として扱い、overlayでは `contentLeftPx/contentTopPx` を一度だけ加算する。別の独自scale、ARIB planeからの再計算、Canvas text layoutを追加しない。

viewportが未確定、幅/高さが0、generation不一致の場合は `aribcc_renderer_set_frame_size()` / renderへ進まず字幕表示成功にしない。同一playback generation内の純粋なviewport size/position変更ではdecoder continuityを壊さない。subtitle schedulerを一旦止め旧bitmapをclearし、renderer frame sizeを新 `contentWidthPx/contentHeightPx` へ更新する。current media timeで安全に再renderできる場合だけ新viewportへ再表示し、できない場合は旧bitmapを拡大縮小して流用せず次の有効captionまでclearを維持する。

### 字幕PTS scheduling / NoPTS / clear ownership

字幕のmedia clockはvideo/audioと同じcurrent MediaSyncのcanonical clockだけとする。TIS、Rust JNI、`CaptionOverlayView`はPCR/wallclockから別media clockを作らず、固定delayや周期的な`getTimestamp()` polling loopも持たない。字幕PESにrendererへ渡せるauthoritativeな33-bit 90 kHz PTSがある場合だけ、video/audioと同じcurrent playback generationのunwrap規則でcanonical `timeUs`へ変換し、その時刻をdecoder/renderer/schedulerの同一caption時刻として使用する。PCR、wallclock、受信時刻、固定delay、直前caption PTS、nominal frame rateから字幕PTSを生成しない。

libaribcaptionの `ARIBCC_PTS_NOPTS` / `PTS_NOPTS` をrendererへappendしない。rendererが使用できるauthoritative PTSがないcaptionは、0へ丸めず、直前PTSをcarry-forwardせず、PCR / wallclock / MediaSync current position / 受信時刻 / nominal frame rateからcaption PTSを生成せず、renderer queueへappendせず、そのcaptionを表示成功として扱わず型付き診断へ記録する。NoPTS入力だけを理由に既に表示中の有効captionを即時clearせず、その既存caption自身のduration / clear / lifecycle契約に従う。現行製品profileが字幕表示対応を表明するには、字幕filter / producerが表示対象caption PESについてrendererに渡せるauthoritative PTSを供給できることをqualification条件に含め、満たせないbackend/profileはdecoderが文字列抽出できてもr51字幕表示成功対応として表明しない。

字幕display/clearは、MediaSyncの`getTimestamp()`が返すmedia time / anchor time / playback rateを唯一の時間基準とするevent-driven one-shot subtitle schedulerが担当する。新caption、finite-duration clear、明示clearのうち次の1境界だけをarmし、予定時刻到達時にcurrent MediaSync timestampを再読して境界到達を確認する。earlyなら同じ境界へre-armし、dueならdisplay/clearして次境界だけをarmする。周期polling、独立free-running clock、PCR→wallclock clock、video frame release/drop判定を実装しない。

libaribcaptionが有限`wait_duration`を返すcaptionは `PTS + duration` を同じcanonical timeline上の明示clear境界とする。`DURATION_INDEFINITE`は有限値へ推測変換せず、次caption、ARIB/libaribcaptionの明示clear、字幕track無効化、generation終了までcurrent imageを保持する。次captionはそのPTS境界で旧imageを直接replaceする。既表示captionのdurationを後から別clockで補正しない。

このschedulerはA/V clockやvideo schedulerを複製するものではなく、MediaSyncが所有するcanonical playback positionに字幕presentation eventを従属させるUI dispatch層である。

### decoder / renderer / scheduler lifecycle

字幕native state、scheduler state、overlay stateは同じpresentation epochに属し、少なくとも次を一組として扱う。decoder queue用generationとUI runnable用epochを別々に所有せず、全失効イベントで単一epochを進める。

```text
CaptionPresentationEpoch:
  playbackGenerationToken
  selectedSubtitleTrackId
  CaptionViewport
  libaribcaption context
  decoder
  renderer
  pending one-shot event
  current rendered frame
```

状態変更はsession/subtitleのserial executor上に直列化し、旧epoch callback/result/eventはepoch不一致で破棄する。

字幕がenabledかつsubtitle trackが選択され、current playback generationと有効viewportが揃った場合だけnative renderer pathをactiveにする。新subtitle generation開始時はcontext/decoder/rendererを既知の初期状態から構築し、renderer initialize後にcurrent viewportで `aribcc_renderer_set_frame_size()` を成功させてからcaption inputを受け入れる。

`onSetCaptionEnabled(false)` はpending scheduler eventをcancelし、overlayを即時clearし、`aribcc_renderer_flush()`相当でrenderer queue/current render stateを失効させる。disabled中のPESを表示用renderer queueへ蓄積しない。再enable時にdisable中に停止・flushされたsubtitle filterのcontinuityを仮定せず、native decoder/renderer state継続可否が証明できない場合は新subtitle generationとして再初期化し、古いdecoder stateの暗黙再利用より再初期化を既定とする。

`onSelectTrack(TYPE_SUBTITLE, null)` は即時にscheduler cancel、overlay clear、renderer flushを行ってcurrent subtitle generationを終了する。別subtitle trackへの変更も旧generationを終了し、新track用context/decoder/rendererを新規初期化して旧trackのcaption/result/eventを持ち越さない。

字幕filter自身のflush、stop/reconfigure/restartによりdata-group continuityが失われ得る場合はpending scheduler eventとoverlayをclearし、rendererをflushし、decoder/rendererを新subtitle generationとして再初期化する。A/V filterだけのplain flushは字幕generationを変更しない。

物理retune、service/codec/PID graph変更、playback generation変更、Surface/MediaSync generation変更では旧subtitle generationを終了し、pending event cancel、overlay clear、renderer flush、decoder/renderer/context解放を行う。新playback generationでは新viewportとtiming epochが確定するまで字幕inputを表示成功にしない。playback rate変更時はcurrent canonical clockに対してpending subtitle eventをcancel/re-armするが、それだけを理由にdecoder stateを破棄しない。

session releaseはpending event cancel、overlay clear、renderer flushの後、renderer → decoder → contextの依存関係を壊さない順で解放し、subtitle executor上のqueued stale workをreleased flag/generation tokenで破棄する。release後にnative callback/resultがUI stateを変更してはならない。

最低試験には、valid viewport確定前のrenderを成功扱いしないこと、valid viewportでRGBA8888 + dst rectがoverlayへ出ること、stride/buffer size検査、viewport変更で旧bitmap座標を流用しないこと、valid PTS captionがcanonical timelineに表示されること、NoPTSを0/前値/PCR/wallclock等で補完しないこと、NoPTS captionをrenderer append/display successにしないこと、NoPTS入力だけで既存有効captionを根拠なくclearしないこと、disable/deselect/track change/subtitle filter continuity loss/retune/playback generation/Surface generation/releaseの各境界で上記state ownershipが成立すること、A/V-only plain flushでは字幕generationをresetしないこと、release後のstale resultが描画しないことを含める。

## ライブ playback 実装方式

TIS のライブplaybackは、Tuner AV filterのclear-memory `MediaEvent.getLinearBlock()`が示す各有効rangeをAOSP標準どおりMediaCodec block modelへ直接queueする。TISはcodec access-unit境界を再解析せず、fragmented ESを別`LinearBlock`へ再構成せず、通常ES payloadをByteArrayへcopyしない。video decoder出力を`MediaSync.createInputSurface()`へ、audio decoder出力PCMを`MediaSync.queueAudio()`へ渡す。MediaSyncはsession SurfaceとAudioTrackを所有し、A/V同期、映像提示時刻、音声clock追従を担当する。TISはMediaSyncの外側に独自clockまたは独自frame schedulerを置かない。

本productはnon-tunneled MediaCodec + MediaSync経路をarchitectureとして採用し、tunneled / platform passthrough playback capabilityを恒久的に提供しない。`notifyVideoAvailable()`は、本書「MediaSync Framework-private final-output observation」で定義したcurrent availability epochのfinal-output成功eventだけをcommitにする。initial generationおよび同instanceでrecoverable unavailableから復旧するepochごとにlistenerをarm/re-armし、decoder output、`OnFrameRendered`、`getTimestamp()`のclock進行だけでは通知しない。

setup scan の channel registration は global discovery complete を必須条件にしない。ただし partial snapshot を無条件に channel insert に使ってはならない。TvProvider のサービス単位の登録可否は本書の「サービス登録・公開・再生policy境界」を唯一の正本とし、この節で video ES 必須などの追加 gate を重複定義しない。したがって `service_type=0x01` は同節の audio-video / video-only 条件、`service_type=0x02` は対応 audio ES を持つ audio-only 条件に従い、`0x02` の登録に video ES を要求しない。登録可能未満の partial snapshot は診断情報 / ライブ更新 / debugにのみ使い、channel insertしない。scrambled サービスはchannel登録してよいが、CAS仮実装のまま平文ライブ視聴成功対応宣言してはならない。

## codec header / A-V sync / publish mode の固定

r51 のライブ playback codec は video=MPEG-2 video / H.264 AVC、audio=AAC / MPEG audio とする。r52では`開発規則.md`の到達点どおり、現行対象の従来TS profileでARIB signaling上HEVC/H.265が現れる場合を同じgeneric Tuner→MediaCodec経路の再生判定対象へ含め、PMT / descriptor認識、MediaFormat、MediaCodec capability照合、decoder起動、MediaSync first-output gate、unsupported診断を同一契約で扱う。codec追加を別の専用再生経路にせず、対象releaseのARIB本文選定規則とdecoder capabilityに従って同じdirect pathへ接続する。

STD-B79のISDB-T2 / ISDB-T1.5およびSTD-B80のISDB-T3は`開発規則.md`で恒久的な製品scope外とされているため、それらの方式だけに依存するcodecを本productのplayback capabilityへ追加しない。

本productがchannel登録およびlive viewableとして対応するARIB `service_type`集合は、`0x01`の`Digital television service`と`0x02`の`Digital audio service`だけに恒久固定する。`TvContract.Channels.COLUMN_SERVICE_TYPE`は`../ARIB_SI_EPG_TvProvider投影方針.md`に従ってARIB `service_type`のcodingを保持し、Android generic `SERVICE_TYPE_AUDIO_VIDEO` / `SERVICE_TYPE_AUDIO`へ意味変換しない。その他のservice typeはparser / provider-data診断ではraw値を保持するが、既知typeへ丸めず`UNSUPPORTED_SERVICE_TYPE`を記録してchannel登録・live viewable対象にしない。

`service_type=0x02`は本来的なaudio-only serviceである。少なくとも1本の現行対応audio ESと物理選局情報、`ServiceKey`、inputId、表示名が揃えば、video ESを要求せずARIB `service_type=0x02`のchannelとして登録し、audio filter・decoder・AudioTrackだけを開始する。視聴sessionでは映像filterを開かず、サービス分類確定後に`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)`を通知し、audio再生の成否と映像なし通知を分離する。audio codec非対応またはaudio ES欠落は`AUDIO_ONLY`の正常理由ではなく、`UNSUPPORTED_AUDIO_CODEC`または`SERVICE_TYPE_PMT_MISMATCH`として再生不能にする。

`service_type=0x01`はaudio-video serviceであり、対象releaseで対応するvideo ESがない場合にaudio-onlyへ再分類しない。弱信号またはlock喪失は`VIDEO_UNAVAILABLE_REASON_WEAK_SIGNAL`、有効なserviceでdecoder起動またはqueue補充を一時待機する場合だけ`VIDEO_UNAVAILABLE_REASON_BUFFERING`、video codec非対応またはPMT構成不整合は`VIDEO_UNAVAILABLE_REASON_UNKNOWN`と型付き診断`UNSUPPORTED_VIDEO_CODEC` / `SERVICE_TYPE_PMT_MISMATCH`へ分離する。HEVCはr51ではmetadata / 診断対象に留め、r52で`開発規則.md`の条件を満たす従来TS profileについてgeneric video playback selectionへ含める。

現行対応 video ES が存在し、audio ES が存在しない、または audio codec だけが現行未対応の場合は、video-only サービスとして視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。STD-B32 4.0以降の改定概要で高度地上デジタルテレビジョン放送向けに追加された MPEG-H 3D Audio / AC-4 は、STD-B79 / STD-B80 の高度地上方式が現行product scope外であるため現行codec固定表へ追加しない。AC-3 / Enhanced AC-3 も現行対象transportに対する条項根拠を確認せず推測で追加しない。

PMTからcodec family、audio/video種別、PIDを確定した後、AV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。snapshotは当該playback generationでTISが保持してよい`MediaEvent`の有限上限として、`singleEventLimitBytes`、`startupQueueBudgetBytes`、`startupQueueMaxSamples`、`startupQueueMaxDurationUs`、`pendingQueueBudgetBytes`、`pendingQueueMaxSamples`、`pendingQueueMaxDurationUs`、`decoderStartupDeadlineMs`、`steadyBackpressureDeadlineMs`を持つ。codec headerをまだ受信していないこと、またはdecoderが未構成であることを理由に開始済みgenerationの値を動的変更しない。値は対象codecと対象decoder/device組合せについてofflineで検証したbuild-time product profile値とし、単一productでは同じ値を別のruntime profile objectへ重複保持することを要求しない。snapshotはTIS側の保持量制御であり、最大上限相当の物理メモリをAV filter開始前に事前確保する契約ではない。

有限な`TisPlaybackBudgetSnapshot`を確定した後にAV filterを開始し、上限内の`MediaEvent`からdecoder構成に必要なcodec configuration metadataだけを取得して`MediaFormat`を構成する。r51ではMPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio headerを対象とし、r52でHEVCを再生判定対象にする場合はHEVC VPS/SPS/PPSも同じheader収集契約へ追加する。header解析に必要な最小rangeだけ`LinearBlock.map()`でread-only参照してよいが、通常payloadのqueue境界を作るcodec parserやES/AU copyへ拡張しない。decoder構成成功後は同じsnapshotのsteady-state上限へ遷移し、startup queueの各`MediaEvent` / `LinearBlock`は元rangeのままblock model QueueRequestへ渡してqueue成功時にcodecへ所有権を移す。runtimeで観測したdecoder block capabilityは各eventの投入可否と製品profile検証の診断にだけ用い、開始済み世代のsnapshotを書き換えない。AOSP direct-input契約を満たさないdecoder/profileではfilterを停止し、保持中のHAL handleを解放して`DECODER_CAPACITY_MISMATCH`または`DIRECT_INPUT_UNSUPPORTED`を記録し、成功対応として表明しない。

MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= LinearBlock capacity`を満たす場合だけstartup queueまたはblock model QueueRequestへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。

TISは保持中の`MediaEvent`についてevent数、payload byte数、presentation timestamp spanをsnapshotの有限上限内に制限する。range検証後にsingle-eventまたはqueue上限を超えるeventは原因別に`SAMPLE_TOO_LARGE`または`PENDING_QUEUE_FULL`を記録してHAL handleを直ちに解放する。保持量はbounded queueから算出しても、同じqueueに従属するO(1) counterで管理してもよく、独立した資源台帳や別generationを設けることを要求しない。generation変更、stop、releaseでは保持中eventを解放して保持量を0へ戻す。TISはAU再構成用の追加bufferを持たず、HALの`avPerFilterLiveBytes`または`avRuntimeBudgetBytes`等のAV backing/resource ledgerを公開・複製しない。

first frame前はcodec-specificな`decoderStartupDeadlineMs`を用い、必要なsequence header、SPS/PPS、audio config、reorder用入力を収集している間の一時queue増加を通常backpressure失敗へ写像しない。startup deadlineまでにdecoder入力可能状態またはfirst frameへ到達できず、queueのbyteまたはduration上限も解消しない場合だけplaybackを停止して`notifyVideoUnavailable()`へ進む。first frame後は別の`steadyBackpressureDeadlineMs`を用い、単発超過は当該sampleを解放して継続し、期限中にdequeue進行がなくqueue上限が継続する場合だけunavailableへ遷移する。audioだけの超過はvideo-only継続可否を既存規則で判定し、無条件にvideo unavailableへ写像しない。

A/V同期方式はAndroid標準`MediaSync`に固定する。本productはnon-tunneled playbackを恒久architectureとして採用し、tunneled playback、platform passthrough、`avSyncHwId`をTIS capabilityとして提供しない。`PlaybackPipeline`のserial executorが、現generationのMediaSync、MediaSync input Surface、session Surface、AudioTrack、video／audio decoder、未返却audio buffer id、playback rateを単一所有する。decoder callback、MediaSync callback、AudioTrack／route callbackはstateを直接変更せず、同executorへ直列化する。

videoは`MediaSync.setSurface(sessionSurface)`の後に`MediaSync.createInputSurface()`を一度だけ呼び、そのSurfaceをvideo decoder出力先とする。decoded outputは元PTSをナノ秒へ変換してMediaSync input Surfaceへrenderする。TISは`AudioPlaybackClock`、`StandalonePlaybackClock`、`VideoFrameScheduler`、`AudioTimestamp.framePosition`由来の独自media position、独自future frame保持、独自late drop閾値、独自renderTimestamp算出を実装しない。

audioはsession固有Contextで作った`AudioTrack`を`MediaSync.setAudioTrack()`へ設定し、audio decoderの現generation output PCMをPTS付き`MediaSync.queueAudio()`へ渡す。block model audio outputの`OutputFrame.getLinearBlock()`は必要範囲をmapし、返されたByteBufferをMediaSyncへ渡す。MediaSyncから`onAudioBufferConsumed(sync, buffer, bufferId)`が返るまで、該当codec output index、OutputFrame、LinearBlock、ByteBuffer、budget claimを保持し、変更・再利用・releaseしない。callback後に対応するcodec outputを非描画releaseし、所有権とclaimを返す。

MediaSyncは生成時のplayback rate 0を用いて必要な有限prefillを行い、視聴制限gate、Surface有効性、decoder開始、最小startup条件成立後に`PlaybackParams`のspeed 1.0で開始する。video-onlyではAudioTrackを設定せずMediaSync video経路を使い、audio-onlyではSurfaceとvideo decoderを設定せずMediaSync audio経路を使う。MediaSync errorは`MEDIASYNC_ERROR_SURFACE_FAIL`と`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`を区別する。surface失敗はvideo経路を持つサービスだけでvideo unavailableへ写像する。audio失敗は、audio-videoサービスでvideo経路を継続可能な場合でも旧MediaSyncを再利用せず、旧AudioTrack/audio decoderを含む現playback generationを終了し、MediaSync・video decoder・video filterを新generationとしてvideo-only構成で再生成する。診断に`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`を残す。audio-onlyサービスでは代替video経路が存在しないためvideo-onlyへ遷移せず、audio decoder、AudioTrack、MediaSyncと未返却bufferを回収して現generationを再生不能状態へ遷移し、`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`は映像が存在しないというサービス属性の通知にだけ使用してAudioTrack失敗理由と混同しない。video-onlyサービスはAudioTrackを設定しないため`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`遷移を持たない。

accepted live `onTune()`、service・codec/PID graph変更、明示的playback generation変更、stop、Surface変更、AudioTrack切替／再生成、audio route変更、decoder再生成では、既存MediaSyncの内部anchorを再利用せず、playback rateを0へ戻して未返却audio bufferと旧decoder outputを回収し、MediaSync input Surface、MediaSync、decoder、AudioTrackを解放して新generationとして再生成する。TISはfrontend lock/healthやchannel同一性を観測してこのfull resetを省略しない。各accepted live `onTune()`でTuner SDKの`Tuner.tune(settings)`を呼び、同一settingsで物理frontendを継続利用できるかはTuner HALの既存frontend tune state machineに委ねる。plain `Filter.flush()`はAOSP契約どおり未消費filter dataのclearに限定し、未queue `MediaEvent` / `LinearBlock`と対応claimだけを破棄してMediaSync/decoder/AudioTrack/playback generationを再生成しない。PTS raw deltaの大小、通常33-bit wrap、presentation-order reorderだけでもMediaSync generationを再生成しない。旧generationのdecoder／MediaSync／route callbackはstate更新に使わず、旧bufferを非描画解放する。

最低試験契約は、AOSP Tunerのclear non-passthrough media filterから得た`MediaEvent.getLinearBlock()`の有効rangeをcodec別AU解析なしでblock model `QueueRequest.setLinearBlock()`へ直接queueすること、ESまたはpartial ESのevent列をTIS側で再構成・copyしないこと、MPEG-2 Video / H.264 / AAC / MPEG audio（r52では対象条件を満たすHEVCを追加）の選択decoder/device profileでこのdirect-input契約を満たすことを確認する。`isPtsPresent=true`では同じeventの`getPts()`だけをevent-level timestampとして使い、offsetをPTS位置と解釈しないこと、`isPtsPresent=false`のeventもpayloadをdropせず、0 / 前値 / PCR / wallclock等からPTSを捏造せず、別eventまたはcodec AUへPTSを再関連付けしないことを確認する。`PtsEpochCoordinator`は`isPtsPresent`をseed / advance / join条件に使わず、producer-authoritative `getPts()`を持つ全non-empty eventで進める。track-local `rawPrev` / `extendedPrev`とgeneration共有wrap epoch、`2^33-1 -> 0` wrap、`0 -> 2^33-1` reorder、半周期差=`-2^32`、generation最初のeventが`isPtsPresent=false`でもauthoritative `getPts()`を持つ場合、後発video/audio trackが`isPtsPresent=false`から開始する場合、generation開始時video=`2^33-100` / audio=`50`と逆順、双方が33-bit wrap近傍で開始する場合、後から開始／置換するtrackのjoinを検証し、A/V timeline差が本来の差を維持することを確認する。PTS deltaの大小だけではgeneration resetしない。plain `Filter.flush()`は未queue event / claimだけをclearし、coordinator / seed済みnormalizer / decoder / MediaSync / AudioTrack / generationを維持する。decoder再生成 / AudioTrack切替・再生成 / audio route変更 / Surface変更 / codec・PID・track graph変更を伴うretuneはfull generation resetにする。`MediaEvent`のlong offset / lengthについて負値、0 length、`Int.MAX_VALUE`境界、`Int.MAX_VALUE+1`、checked-add overflow、end==capacity、end>capacityをnarrow前に検査し、通常payloadをByteArray・別LinearBlock・通常input-bufferへcopyしないことを確認する。MediaSync rate-0有限prefillからstartup gate成立後speed 1.0へ遷移すること、`MEDIASYNC_ERROR_SURFACE_FAIL`と`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`の分離、audio-videoでAudioTrack failure時に旧MediaSync generationを破棄してvideo-only新generationへ再生成、audio-onlyでの再生不能遷移、video-onlyがAudioTrack error遷移を持たないこと、各accepted video `onTune()`でfresh availability armを作り、そのarm後のfinal-output成功前はvideo availableにしないこと、late-drop / attach失敗 / queue失敗でcurrent armを消費しないこと、成功event後だけ一回availability通知すること、`available -> recoverable unavailable -> available`で同MediaSync instanceを維持する場合にfresh armでre-armして次のfinal-output成功後だけ再availableにすること、arm Aの成功event配送を遅延させたままunavailable遷移とarm Bのre-armを行い遅延Aをarm sequence不一致で破棄すること、generation teardown後は新instanceのinitial armを使うこと、audio bufferのconsume callbackまでの寿命、A/V同期、video-only、audio-only、destructive retune / Surface変更 / AudioTrack切替・再生成 / audio route変更 / decoder再生成後のMediaSync再生成、各accepted `onTune()`で新playback generationを作り、そのgenerationのfresh initial arm後のfinal-output成功でavailabilityを通知すること、同一MediaSync instance内で旧arm sequenceを再利用しないこと、route変更後の旧generation非利用、MediaSync error写像、stale generation非描画を含む。試験のqueue数値上限は選択した`ProductProfile`と一致させる。

TvProvider公開モードは `PublishMode` で channel row 追加を setup scan / explicit rescan に限定する。ライブ tune refresh、boot EPG sync、background channel maintenance では既存 channel の番組・診断更新だけを許可し、新規 channel row は追加しない。

## ARIB SI/EPG のTvProvider投影

ARIB SI/EPG の標準列投影は tv直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode は `arib_si_engine_rs` の Rust provider-data serde構造体を SSOT とする。TISは、同文書で標準列投影が固定された項目だけを TvProvider 標準列へ出し、標準列へ自然対応しないARIB意味情報は JSON v1 `internal_provider_data` のみに構造化保存する。TIS/product policy結果やruntime track identityはinternal_provider_dataへ戻さない。

`Programs.COLUMN_CANONICAL_GENRE` については、TIS が直接設定する値と、Android TvProvider が `Programs.COLUMN_BROADCAST_GENRE` から内部補完した読み出し結果を区別する。現行仕様では `ARIB_SI_EPG_TvProvider投影方針.md` の明示写像表に一致する分類だけを `ContentValues` に直接設定する。写像不能分類、reserved、extension、others、user_nibble 由来分類は直接設定しない。canonical genre投影結果をRust provider-dataへ保存しない。

`Programs.COLUMN_BROADCAST_GENRE` には、`arib_si_engine_rs` から受け取った ARIB content_descriptor の分類値とARIB表示名を、TIS が `TvContract.Programs.Genres.encode(...)` 形式で格納する。TIS は ARIB分類を Android canonical genre に推測変換しない。

## 視聴制限 / コンテンツレーティング契約

TIS は `arib_si_engine_rs` から受け取った `parental_rating_descriptor` の構造化データを、AOSP system-defined ISDB レーティングドメイン（`com.android.tv / ISDB / ISDB_<age>`）の `TvContentRating` へ変換する。Android `TvContentRating` の domain / ratingSystem / レーティング文字列は TIS 側で固定し、Rust 側のSSOTまたはprovider-dataへ戻さない。

TvProvider へ番組を登録または更新する場合、変換できるレーティングは `TvContentRating.flattenToString()` の結果を `Programs.COLUMN_CONTENT_RATING` に格納する。変換できないレーティングは推測で `COLUMN_CONTENT_RATING` に入れず、ARIB raw値とparse状態を`internal_provider_data`と診断に保持する。

ライブセッションは、現在番組のレーティングとsystem視聴制限設定を同期して扱う。`TvInputManager.isParentalControlsEnabled()` が true の場合、TIS は現在番組の `TvContentRating`、またはレーティング未取得時の `TvContentRating.UNRATED` を `TvInputManager.isRatingBlocked(...)` に渡して判定する。blocked の場合は video frame を表示する前に再生を停止または抑止し、`notifyContentBlocked(rating)` を呼ぶ。許可された場合は `notifyContentAllowed()` を呼ぶ。

TIS は `TvInputManager.ACTION_BLOCKED_RATINGS_CHANGED` と `TvInputManager.ACTION_PARENTAL_CONTROLS_ENABLED_CHANGED` を監視し、設定変更時に現在番組の視聴制限判定を即時再評価する。

## TIS/arib_si_engine_rs 固定事項

- LineageOS 21／Android 14の通常ライブセッション生成では`onCreateSession(inputId, sessionId, tvAppAttributionSource)`をoverrideする。framework由来`sessionId`は`Tuner(serviceContext, sessionId, useCase)`へ渡し、`tvAppAttributionSource`はsession固有Contextの生成へ渡す。2引数版`onCreateSession(inputId, sessionId)`と1引数版は明示的な互換経路だけに限定し、対象productの通常3引数入口を素のservice Contextへ委譲または後退させない。
- r51 の video 対応宣言対象は MPEG-2 video `0x02` と H.264/AVC `0x1b` とする。HEVC `0x24` はr51ではmetadata / 診断へ保持し、r52で`開発規則.md`の到達点どおり、現行対象の従来TS profileでARIB signaling上現れる場合をgeneric Tuner→MediaCodec playback selectionへ含める。r52のHEVC対応は規範対象の現行ARIB原文、検証証拠の版・条項と未証明差分、MediaFormat / decoder capability / first-output gate / unsupported診断を同じ契約で固定する。
- ARIB 視聴年齢制限は raw `parental_rating_descriptor.rating` の意味を保ったまま Android `TvContentRating` へ写像する。`country_code=JPN` の `0x01..0x0F` は `age=raw+3` で AOSP system-defined `com.android.tv / ISDB / ISDB_4..ISDB_18`、BS/CSで運用される `0x10..0x11` は同式で `ISDB_19..ISDB_20` とする。明示的に受信した `0x12..0xFF` は年齢へ推測変換せず、product rating provider が定義する `com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` へ写像する。`0x00` と未対応countryはAndroid ratingを捏造せずraw値と診断に残す。`TvContentRating.UNRATED` は TvProvider current Program と latest EIT の双方から現在コンテンツに適用可能なratingが得られない場合だけに使用し、明示的な `0x12..0xFF` の代替値にはしない。
- `notifyVideoAvailable()` はcurrent MediaSync availability epochでlate-dropを通過しcurrent final outputへのattach＋`queueBuffer()`成功後に発行されるFramework-private first-output eventを受け、current Surface有効、generation一致、視聴制限、Surface errorのgateを満たした場合だけ呼ぶ。recoverable unavailableから同MediaSync instanceで復旧する場合はlistenerをre-armし、次の成功event後に再度availableへ遷移する。decoder output、`OnFrameRendered`、`getTimestamp()`のclock進行、drop、旧instance callbackはavailability根拠にしない。
- ライブ tune refresh では新規 channel row を作らず、既存 channel の program 更新だけを行う。setup/rescan のみ channel row を作成できる。
- H.264 は SPS/PPS 検出だけでなく SPS 由来の width / height を MediaFormat へ反映する。SPS 解析不能時は固定 1920x1080 代替処理で成功扱いしない。
- PMT 由来の video/audio/subtitle track は `TvTrackInfo` として通知し、TIS runtimeがcomponent identityから`trackId`を生成して `onSelectTrack(TYPE_AUDIO, trackId)` 等へ接続する。`trackId`はsession/runtime identityであり、ARIB SI意味データやprovider-dataへ保存しない。現行 product では字幕 track と libaribcaption 表示経路を実装対象に含める。別 video track と data track 選択は、対応 codec / 実行環境がない限り対応宣言しない。
- CS110 は stream selector `NONE` のみ許可し、TSID / relative selector を HAL tune request へ渡さない。Android Tuner builder では NONE 時に selector setter を呼ばない。
- boot 後 EPG 再同期は既存 channel の p/f 最小更新に限定し、新規 channel row は作成しない。`JapanIsdbScanPlan.defaultInitialScan()` は setup scan / explicit rescan 専用であり、boot EPG sync の既定候補に使わない。
- background channel maintenance は現行スコープ内の必須実装とする。ただし boot critical path から分離し、boot EPG sync 完了後または明示的保守タイミングで実行開始を試行する。実行開始は scan/maintenance が未実行で、かつライブセッションが存在しない場合に限る。active ライブセッションまたは scan 実行中の場合は開始せず、skip 理由を診断情報に残す。対象は既存 channel と既存 transport メタデータ refresh までに限定し、新規 channel insert は行わない。
- セクションフィルターはCRC protected sectionで`setCrcEnabled(true)`を使用し、Rust側CRC検査をdefense-in-depthとして維持する。TIS側にはPID / table / 状態別counterを持つ。


## 視聴年齢制限 / CAS current-state固定

- `Programs.COLUMN_CONTENT_RATING` と Live session の視聴制限判定は同じ `AribRatingMapper` を使う。JPN raw `0x01..0x11` は AOSP system-defined `com.android.tv / ISDB / ISDB_4..20`、明示的な JPN raw `0x12..0xFF` は product rating provider の `com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` へ写像し、後者を `TvContentRating.UNRATED` へ潰さない。
- `MaleicacidTvInput` APK自身はrating-system XML / receiverを所有しない。productは独立した `AribContentRatings` APKを `/product` に組み込み、TIF標準 `ACTION_QUERY_CONTENT_RATING_SYSTEMS` / `META_DATA_CONTENT_RATING_SYSTEMS` 機構でexceptional ratingを公開する。このAPKはpublic APIだけで成立させ、platform certificateやprivileged permissionを要求しない。
- System TV Appはpolicy ownerとして、parental controlsが有効でglobal policyが`NONE`以外の場合に限り上記exceptional ratingをblocked-rating集合へ反映する。PIN認証済みcurrent contentの `onUnblockContent()` 一時解除は維持し、第三者custom rating、CTS Verifier由来rating、他domain/ratingSystemへこのproduct policyを波及させない。TISはraw値から独自policyを実装せず `TvInputManager.isRatingBlocked()` の結果だけに従う。
- Live session は現在番組ratingを `TvProvider current Program -> latest EIT cache -> TvContentRating.UNRATED` の順で解決する。ただし前二者からexceptional ratingを含む適用可能ratingが得られた場合はそれを使い、`UNRATED` はrating情報が得られなかった場合だけのfallbackとする。
- parental blocked の通知は `notifyContentBlocked(rating)` と AV停止を主とし、parental block の通知手段として `notifyVideoUnavailable()` を呼ばない。
- `onUnblockContent()` の解除範囲は同一 `channelUri + serviceKey + eventId + ratingString` の現在番組 / レーティングに限定する。start/end は stable identity ではなく、解除対象が現在表示中の同一 Program row であることを確認する補助条件としてのみ使ってよい。start/end/duration を provider-data `programKey`、unblock stable identity、または Program identity の SSOT にしてはならない。
- CAS 未完成 / scrambled unsupported で再生成功にしない場合は `TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` を使う。具体的な CAS 状態 reason は CAS HAL 本実装まで使わない。
- `requiresCas`はcurrent `ServiceSemanticFacts`のCA descriptor等から得る放送由来意味事実とし、`unsupportedCas` / `clearLivePlaybackSupported`はcurrent product/CAS capabilityからTISがその都度算出する。既存channel/Program `internal_provider_data`の旧policy値をcurrent policyの代替参照に使わない。

## TIS / EPG 公開境界

現行の EIT publish/delete 対象は、TvProvider に channel が存在する `ServiceKey`、または同一 setup/rescan transaction で channel insert が成功して channelId が確定した `ServiceKey` に限定する。ライブセッション の `currentService` だけには限定しない。Program row を持たないサービスへ Programs を publish/delete してはならない。

現行r51の EIT publish/delete 対象 table は present/following actual `0x4E` のみとする。present/following other `0x4F`、schedule actual `0x50..0x5F`、schedule other `0x60..0x6F` は r51 の Programs publish/delete 対象外であり、更新区間を発生させない。r53以降で対象を拡張する場合は `開発規則.md` のrelease scopeを先に更新する。

EIT 更新時の update/削除区間は、追加・変更・削除された event の既存 `[start,end)` と新 `[start,end)` の union とする。現行仕様では長期固定 lookahead window を導入しない。長期 EPG lookahead window を扱う場合は、EIT scope / version / event identity / authoritative 条件を設計正本へ固定してから併用する。EIT table scope の version 変更で既存 section が消えた場合は、消えた event の既存 window も廃止行削除対象に含める。

ただし、廃止行削除の根拠にできる EIT section / table snapshot は Rust parser が `deletionAuthoritative=true` と判定したものに限る。start_time BCD、duration BCD、event descriptor_loop_length、event fixed フィールドが malformed の event を含む section は、既存 event 削除用の authoritative valid-event-set として扱わない。malformed event は既存正常 Program を消す根拠にせず、DescriptorDiagnosticV1 / ParserDiagnosticV1 に記録する。

Direct Boot保留の正式状態を`DirectBootEpgPending`とする。`DirectBootGuard`がdevice-protected storage上のこの状態を唯一所有し、boot EPG sync要求を受理した時点または未完了・失敗終了時に設定する。`ChannelScanManager`はJobSchedulerのschedule/cancelだけを担当し、pending、inputId、Contextのshadow stateを持たない。JobServiceは開始時に自TISのinputIdを再解決する。状態はprocess restartとuser unlockをまたいで保持し、background maintenanceは設定・解除しない。

`BootEpgSyncCoordinator` は Tuner や SI collection を開始する前に、解決済みの自 TIS `inputId` を使って既存 `TvContract.Channels` を必須問い合わせとして取得し、今回の boot EPG sync の authoritative target channel 集合を確定する。この必須問い合わせ自体が失敗した場合は channel なしとは扱わず `DirectBootEpgPending` を維持して再試行対象にする。問い合わせが正常終了し、自 TIS 所有の既存 channel が 0 件だった場合は、boot EPG sync に更新対象が存在しない `NO_WORK` 正常終了とする。この場合は Tuner、SI collection、Programs publish/delete を開始せず `DirectBootEpgPending` を解除し、JobScheduler の再試行を要求しない。setup / explicit rescan はこの `NO_WORK` 判定とは独立した channel 登録経路であり、boot EPG sync は 0 件状態から channel を作成しない。

既存 target channel が 1 件以上ある場合は、同一boot EPG sync taskがcancelされず、`collectSiForCandidate()`が`COMPLETE`となるcandidateを1件以上得て、対象channel／Programに必要なTvProvider必須問い合わせとinsert/update/deleteが一つのpublish transactionとして全て成功したcommit後にだけ`DirectBootEpgPending`を解除する。provider query/write failure、publish fingerprint生成失敗、cancel、target channel が存在するのに登録可能サービスまたはpublish可能Programが0件となった場合は保留を維持する。candidate成功だけ、部分write、またはfingerprint cache更新だけを解除根拠にしない。したがって `NO_WORK` は「開始前のauthoritative channel queryが正常終了し、その結果が0件」の場合だけであり、受信失敗やSI不完全、policy不足を0件成功へ丸めない。

最低試験は、(1) authoritative channel query failure では Tuner を開始せず pending を維持して再試行すること、(2) query 成功かつ自TIS所有channel 0件では Tuner / SI collection / Programs publish-delete を開始せず `NO_WORK` として pending を解除し再試行しないこと、(3) target channel が1件以上あるが全candidate失敗またはpublish可能対象0件の場合は pending を維持すること、(4) target channel が1件以上ありpublish transactionが正常commitした場合だけ通常成功としてpendingを解除すること、を含める。

登録可能サービスは、`ServiceKey`、物理選局情報へ戻せるchannel provider-data、`Channels.COLUMN_INPUT_ID`として保存する自TISのinputId、表示名が揃い、TvProvider channel insert/update に進めるサービスとする。input ownershipのSSOTはprovider-dataではなく`Channels.COLUMN_INPUT_ID`とする。表示名は `ChannelRecord.displayName` が nonblank ならそれを使い、なければ SDT service_name、さらに無ければ `service-<onid>-<tsid>-<sid>` を使う。この代替表示名は登録可能判定上の有効な表示名と扱う。

## CAS 仮実装境界

CAS HAL 仮実装のまま scrambled サービスを平文ライブ視聴再生成功として扱ってはならない。scrambled unsupported サービスでも、PMT/CAT/CA情報と診断を使って EPG / Programs / レーティング / provider-data は更新する。ただし CAS key トークンを提供できない状態では再生成功にせず、CAS起因の unavailable のみ `VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` へ map する。初回映像到達timeout、filter start failure、非対応stream、codec失敗、audio失敗はCAS unknownにmapしない。

CAS可否はcurrent `ServiceSemanticFacts`とcurrent CAS implementation/capabilityからTISが算出する。provider-dataに保存するのはCA descriptor/free_CA_mode等の意味事実だけであり、旧`unsupportedCas` / `clearLivePlaybackSupported` / `publishStateSource`をcurrent判定へ再利用しない。

Descrambler API の `setKeyToken()`、`addPid()`、`removePid()` は戻り値が `Tuner.RESULT_SUCCESS` の場合だけ成功とする。非 SUCCESS result は CAS 診断 failure として扱い、成功扱いで握り潰してはならない。

## TvProvider failure semantics

TvProvider query failure と channel なしは別状態として扱う。既存 channel query が失敗した場合は `skippedNoChannel` として扱わず、failure診断とし、publish fingerprint更新・`DirectBootEpgPending`解除の根拠に使わない。

TvProvider query は必須問い合わせと任意問い合わせを区別する。チャンネル・番組の追加または更新、廃止行削除、既存チャンネル・番組検索、Direct Boot準備完了判定に使う query は必須問い合わせとする。必須問い合わせで `ContentResolver.query()` が null cursor を返した場合は `TvProviderQueryFailure` とし、empty resultとみなさない。`TvProviderQueryFailure` が発生したサービス/windowでは channel insert、program insert/update、廃止行削除、publish fingerprint cache更新、`DirectBootEpgPending`解除に進まず、再試行区間を保持する。provider-dataはcurrent policyのfallback sourceにしないため、policy判定のためのprovider-data代替参照queryを設けない。

Programs publish/delete が provider failure になった場合は、`ProgramPublishCoordinator` の process-local dirty-window queue に `ServiceKey + updateWindow` をkeyとしてenqueueする。entryが持つ実行制御値はauthoritative windowと`notBeforeMs`だけとし、failure classは診断値に限定する。固定cooldownは60秒とする。次回 `publishLiveProgramsForCurrentService()`、boot EPG sync、background maintenance のpublish entrypoint先頭で、`now >= notBeforeMs`のentryだけを実行対象としてdrainする。entrypointが来ない限り時刻到達だけでwake-upしない。成功したkeyは削除し、失敗したkeyは同じ固定cooldownで末尾へ戻す。attempt段階、jitter、retention timer、failure class別queueを設けない。process restartではqueueを破棄し、boot/background syncによる再収集を正とする。provider failure時は廃止行削除、publish fingerprint更新、`DirectBootEpgPending`解除に進まない。

dirty-window queueは全体上限512 windowsの単一LRUとする。超過時は最古entryを破棄し、ServiceKey別`droppedRetryWindowCount`を加算する。ServiceKeyごとの第二上限は設けない。process restart後はcounterを0に戻す。

SDT-other / NIT-other / BAT 由来で現在 candidate の actual transport に解決できないサービスは、現在 candidate の物理情報で channel insertしない。未登録で Program row が存在しない unresolved transport は scan/maintenance 診断情報に `skippedUnresolvedTransportCount` として記録し、Program provider-dataには書かない。unresolved transport はTISのpublish policy上の結果であって放送由来のProgram意味情報ではないため、解決済み・publish済みProgramについてもその否定値をprovider-dataへ保存しない。unresolved情報だけを根拠に現在candidateのONID / TSID / 物理情報を補完してChannel / Programを生成・更新せず、既存rowの失効・削除は通常のauthoritative snapshot契約だけに従う。

## provider-data 利用境界 / publish fingerprint

`Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` の具体schema、正規化、安定キー抽出、保存上限は `arib_si_engine_rs/DESIGN_JA.md` の「provider-data / 診断情報 Rust SSOT」と `arib_si_engine_rs/schema/*.schema.json` を正とする。TIS は保存schemaを再定義しない。

TIS Kotlin は provider-data JSON を `JSONObject.put()` や文字列連結で直接構築してはならない。TIS Kotlin は Rust JNI の build / 正規化 / key extraction API で得たbytesをTvProviderに書く。TIS が JNI へ渡す JSON は Rust builder への入力 DTO であり、TvProvider に保存する provider-data schema ではない。

Program provider-data の top-level envelope、必須フィールド、検証規則、正規化、安定キー抽出は TIS では再定義しない。正本は `arib_si_engine_rs/DESIGN_JA.md`、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json`、`arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json` とする。TIS instrumentation テスト用の期待値 JSON を置く場合は Rust 側テストデータとバイト単位で同一に保つ。

TIS は `components.video[]`、`components.audio[]`、`components.subtitle[]`、`components.data[]` を provider-data schema として再定義しない。TIS は TvProvider 標準列、`TvTrackInfo`、MediaFormat / AudioTrack / 字幕表示経路へ接続する接着層に限定する。runtimeで選択したmain audio/video要約や`trackId`をprovider-dataへ戻さない。

Program publish fingerprintは、同一process内で同じ公開transactionをTvProviderへ重複書き込みしないためだけに使用する。TvProviderへ実際に書く`ContentValues`（provider-data bytesを含む）と更新windowを、固定column list順の`<columnName>\0<byteLength>\0<bytes>`へ直列化し、そのSHA-256 lowercase hexをprocess-local cacheにだけ保持する。TvProvider rowやprovider-dataには保存せず、診断、真正性、改ざん検出、永続identityには使用しない。insert後にprovider-dataを再生成した場合は、実際に書いた最終bytesからfingerprintを再生成する。この行全体fingerprintがprovider-data bytesの同一性も包含するため、provider-data単体のdigestは生成しない。

publish fingerprint は、provider-data bytesを含む TvProvider へ実際に書く最終 `ContentValues` と更新windowだけを固定column順で直列化して計算し、JSON key単位の除外規則を設けない。TvProvider row id に依存する診断値を provider-data へ混ぜないことで、row作成後の診断更新が fingerprint を自己参照的に変更する構造を禁止する。


## 現在番組選択

現在番組 resolver は TvProvider query 時点で `START_TIME_UTC_MILLIS <= now AND END_TIME_UTC_MILLIS > now` に絞る。sort order は `START_TIME_UTC_MILLIS DESC, END_TIME_UTC_MILLIS ASC, _ID DESC` に固定する。overlap がある場合も cursor 返却順には依存せず、この selection rule で1件を選ぶ。

現在番組選択の診断は process-local `CurrentProgramResolutionDiagnostic` とし、`selectionRule`、`overlapCount`、`selectedProgramId` を保持できる。`selectionRule` は `START_DESC_END_ASC_ID_DESC` とし、対象なしの場合は empty string とする。この診断は `Programs.COLUMN_INTERNAL_PROVIDER_DATA` へ永続化せず、publish fingerprint、Program identity、unblock identity の構成要素にしない。ARIB `event_id` は `COLUMN_EVENT_ID` と JSON v1 `programKey.eventId` で扱う。

## CA descriptor / provider-data 直列化

CA_descriptor の raw bytes は Rust parser が元 section から保持し、JNI snapshot DTO に raw bytes として渡す。Kotlin 本番経路 code で CA_descriptor を再構築しない。malformed CA_descriptor は元記述子 / CASメタデータから除外し、サービス自体は保持する。診断情報には `malformedCaDescriptorCount` と table/PID/サービス context を残す。Kotlin側で修復してprovider-dataやCASメタデータに不正な元記述子を入れてはならない。

malformed CA_descriptor の詳細診断は、CAS検出snapshotまたはサービス / channel provider-data診断を一次保存先とする。Program provider-dataには、そのProgram公開時点で参照したservice / CAS意味診断のsummaryとして`malformedCaDescriptorCount`を保存してよい。ただしraw descriptor、table/PID/サービスcontextの完全情報をProgramごとに重複展開してはならない。Program側summaryはCASメタデータや再生可否判定の根拠ではなく、公開時点の診断参照結果として扱う。

## transaction DTO / provider-data SSOT / executor / setup / retry の固定

### Rust JNI provider-data API

TIS Kotlin は provider-data JSON を解釈せず、以下の Rust JNI API 相当だけを使う。

```kotlin
object NativeProviderData {
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
    val requiresCas: Boolean,
)
```

`ChannelTune` は `deliverySystem`、`frequencyHz`、`streamIdType`、`streamId`、`physicalChannel`、`satelliteBand`、`remoteControlKeyId` だけを持つtyped物理tune復元値とし、`inputId`、表示名、backend名、driver名、px4相対slot等を持たない。channelとTvInputServiceの関連付けはchannel rowのrequired fieldである`TvContract.Channels.COLUMN_INPUT_ID`を唯一のSSOTとする。tune復元前にrowの`COLUMN_INPUT_ID`がcurrent TISの`TvInputInfo.id`と一致することを検証し、不一致rowのprovider-dataを別inputの物理tuneとして使用しない。`decodeChannelProviderData()` は invalid UTF-8、malformed JSON、schema不整合を null または診断付き失敗へ落とす。現行String JNI surfaceではtyped resultを単一JSON envelopeで返し、Kotlinはこのresult envelopeだけを読む。保存済みprovider-data自体の解釈・修復やTAB/hexの第二wire protocolは設けない。

`inputJson` はChannel builderだけの入力 DTO であり、TvProvider に保存する provider-data schema ではない。Program provider-dataはRustが同一SI transactionのevent / service factsから直接canonical encodeし、event DTOの`providerDataCanonicalJson`として返す。Kotlinはこのfieldを不透明なUTF-8 JSON文字列として`ProgramRecord`へ運び、`Programs.COLUMN_INTERNAL_PROVIDER_DATA`へ保存する。最終JSONバイト列、正規化、安定キー抽出はRustが行う。provider-data単体のdigestまたはsignatureは返さない。

`rawBytes` は任意バイナリではなく、既存 TvProvider に保存済みの JSON v1 UTF-8 バイト列を指す。Kotlin は `String(rawBytes)` などで再解釈してから Rust へ渡してはならず、TvProvider から取得した `COLUMN_INTERNAL_PROVIDER_DATA` の BLOB バイト列をそのまま Rust JNI 境界へ渡す。TvProvider が文字列として返した場合の互換補助は、UTF-8 バイト列へ戻すだけに限定し、Kotlin側でJSON構造を解釈・再構築してはならない。

`normalizeProgramProviderData(rawBytes)`、`extractProgramKey(rawBytes)`、`decodeChannelProviderData(rawBytes)`は、invalid UTF-8またはmalformed JSONをKotlin側で修復しない。Rustは診断付き失敗、key抽出失敗、またはchannel decode失敗へ落とし、通常実行経路で例外やpanicに変換しない。provider-data bytesだけのdigest APIと`ProviderDataResult.signature` / `contentDigest`は設けない。

### 診断情報 schema

Descriptor診断の機械検証規則は `arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json` を正とする。TIS は `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` 配下のオブジェクトを別 schema へ変換せず、Rust JNI が返した provider-data JSON 内の診断情報を保存する。ARIB視聴年齢制限は`ratings[]`にraw構造化値を残し、Android対応可否や写像結果はprovider-dataへ戻さない。TIS Kotlin は descriptor diagnostic JSON を独自生成しない。

### provider-data 保存上限

provider-data の soft limit / hard limit、診断情報・長文補助情報の切り詰め規則、切り詰め時の診断 key は `arib_si_engine_rs/DESIGN_JA.md` と Rust provider-data 実装を正とする。TISは保存前にRust JNIが返したbytesをそのまま扱い、Kotlin側で独自の切り詰めschemaを定義しない。

### SectionEvent 入力上限

TIS の PSI/SI section path は allocation 前に `SectionEvent.dataLength` を検証する。`MAX_SECTION_BYTES` は 4096 bytes とし、`dataLength` が 1..4096 の範囲にある場合だけ ByteArray 確保と `AribSiEngine` / CAS / Program publish への投入を許可する。section read size 不一致、0 length、負値相当、4096 bytes 超過は parser に渡さず診断カウンターに記録する。

### transaction DTO API

`AribSiEngine` 呼び出し側は複数 snapshot を合成してはならない。本番経路は以下の用途別bulk DTOを使う。engineから受け取るpolicy入力は`ServiceSemanticFacts`だけであり、`ProgramPublishability`等のTIS product policyをRust側DTOに持たせない。

```kotlin
data class ProgramPublishSnapshot(
    val ingestSequence: Long,
    val events: List<AribEvent>,
    val updateWindows: List<EpgUpdateWindow>,
    val serviceFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts>,
    val descriptorDiagnostics: List<DescriptorDiagnostic>,
    val parserDiagnostics: List<ParserDiagnostic>,
    val malformedCaDescriptorCountByServiceKey: Map<ServiceKey, Int>,
)

fun takeProgramPublishSnapshot(): ProgramPublishSnapshot
```

`AribEvent.providerDataCanonicalJson`は同じbulk read内でRustが同じevent / service / semantic facts / descriptor診断から生成した保存bytesである。標準列投影用の構造化fieldと用途は異なるが、意味stateとtransaction ownerは一つである。Kotlinが構造化fieldからProgram provider-data requestを再構築し、JNIへ戻してはならない。

```kotlin
data class TableRequirementStatus(
    val component: String,
    val originalNetworkId: Int?,
    val transportStreamId: Int?,
    val serviceId: Int?,
    val required: Boolean,
    val complete: Boolean,
)

data class ServiceRegistrationSnapshot(
    val discoveryStage: Int,
    val tableRequirements: List<TableRequirementStatus>,
    val services: List<AribService>,
    val actualTransports: Set<TransportKey>,
    val actualTransportMetadata: List<AribTransport>,
    val serviceFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts>,
    val diagnostics: List<ParserDiagnostic>,
)

fun serviceRegistrationSnapshot(): ServiceRegistrationSnapshot
```

```kotlin
data class CasDiscoverySnapshot(
    val services: List<AribService>,
    val caMetadata: List<CaMetadata>,
    val pmtPids: Map<ServiceKey, Int>,
    val catEmmPids: List<Int>,
    val diagnostics: List<DescriptorDiagnostic>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic>,
)

fun casDiscoverySnapshot(): CasDiscoverySnapshot
```

`ingestSequence`はsection ingestにより意味stateが更新された順序であり、snapshot read回数ではない。readするたびに増える`snapshotGeneration`は設けない。discovery stage、table requirement status、services、CA、diagnosticsは一回取得した同じimmutable native transactionから用途別DTOへ投影し、stageやCAS用serviceを別JNI readで再取得しない。

`MalformedCaDescriptorDiagnostic` は、少なくとも `pid`、`tableId`、`tableIdExtension`、`serviceId`、`elementaryPid`、`scope`、`offset`、`declaredLength`、`actualRemainingLength`、`reason`、`rawPrefixHex` を持つ。詳細診断の一次保存先は CAS discovery snapshot とし、Program provider-data は `malformedCaDescriptorCount` summary だけを保存する。

`takeProgramPublishSnapshot()` は events / updateWindows / service semantic facts / 診断情報を同一ロック / 同一 native state から取得し、updateWindows の drain もこの API 内だけで行う。`snapshotEvents()` と `takeEpgUpdateWindows()` を本番経路呼び出し側で別々に呼ぶことは禁止する。LiveSessionの現在番組判定、視聴年齢制限判定、映像メタデータ補完のようにupdateWindowsを消費してはならないread-only参照は`programStateSnapshot()`を使い、drain型stateを返してはならない。

廃止 snapshot wrapper は本番経路・公開通常境界・product build に残してはならない。テスト専用に必要な入口は test source または test-only 可視性に隔離し、本番 APK / JNI API / release API から参照不能にする。

### LiveSession / PlaybackPipeline / Scan の直列化

`MaleicacidLiveSession` は session-level serial executor を持ち、currentサービス、generation、track state、unblock state、latest videoメタデータ、`ProgramPublishCoordinator`へのアクセスを同一executorに閉じる。AV開始lifecycleはSessionが`Idle / Starting(signature) / WaitingFirstOutput(signature,generation) / Started(signature,generation) / Failed(signature,generation?) / Stopped`のsealed stateを一つだけ所有する。current/pending signature、last attempted/started gate、pipeline generationを並行して保持しない。遷移判定は状態を持たない純粋関数とする。TunerController、PlaybackPipeline、parental receiverのコールバックは直接state mutationせず、session executorにenqueueする。

`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。filter、block model decoder、MediaSync、MediaSync input Surface、AudioTrack、generation、surface、未返却audio buffer id、availability arm sequenceの変更を呼び出し元スレッドで直接行わない。release後のqueued taskはreleased flagとgenerationで破棄する。

`ChannelScanManager` は`ActiveScanTask(generation, purpose, context, cancelRequested, controller, engine)`を一つのatomic referenceとして所有する。running boolean、active generation/purpose、controller、engine、contextを別fieldに複製しない。cancel / cleanup taskは取得した同じtask identityにだけ作用し、stale cleanupが後続scanを変更してはならない。Tuner Framework/TRMにはsetup scanで`PRIORITY_HINT_USE_CASE_TYPE_SCAN`、boot EPG同期とbackground maintenanceで`PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND`、liveで`PRIORITY_HINT_USE_CASE_TYPE_LIVE`を渡し、frontend等のhardware arbitrationを再実装しない。一方、ライブ中はboot/background作業の開始を延期するというTIS製品policyだけはManagerに残す。

### SetupActivity 保護

`SetupActivity.onCreate()` は scan を自動開始しない。scan 開始前に正規 setup flow の inputId が自 TIS の inputId と一致することを検証する。inputId 欠落または不一致時に代替inputIdでscanへ進まない。scanは検証済みユーザー操作または検証済みsetup requestの後に開始する。

product側でシステムTVアプリにgrant可能な場合、SetupActivityは署名 / privileged permissionで保護する。permission grantが成立しないtargetでも、自動scan禁止、inputId検証、ユーザー操作開始は必須とする。

SetupActivity は自分が開始した `SETUP_SCAN` purpose かつ同一 scan generation の Completed だけで `RESULT_OK` にする。過去の Completed、boot EPG sync、background maintenance の Completed で finish してはならない。

### Direct Boot の保留処理とライブセッションの優先順位

`MaleicacidTvInputService.onCreate()` は Direct Boot の保留処理、起動時の EPG 同期、定期保守を直接開始しない。起動通知を受ける `BootReceiver` は `android.permission.RECEIVE_BOOT_COMPLETED` を宣言したうえで `ACTION_LOCKED_BOOT_COMPLETED` と `ACTION_BOOT_COMPLETED` を受信する。`ACTION_LOCKED_BOOT_COMPLETED` では `DirectBootEpgPending` の記録だけを行い、TvProvider、Tuner、JNI 経由の解析処理は起動しない。`ACTION_BOOT_COMPLETED` は利用者のロック解除後の正規の起動時入口とするが、この通知単独を無条件の再開保証とはしない。状態の正本はデバイス保護領域の `DirectBootEpgPending` とする。

`BootReceiver.onReceive()` は保留状態を確認し、必要なら Android 標準の `JobScheduler` に固定識別子の `BootEpgSyncJobService` を登録するところまでで終了する。EPG の収集、Tuner の使用、TvProvider への反映処理は `BroadcastReceiver` の実行時間へ結びつけず、`android.permission.BIND_JOB_SERVICE` で保護した `BootEpgSyncJobService` の実行寿命下で行う。起動時 EPG 同期用の `JobInfo` は再起動をまたいで永続化せず、再起動をまたぐ正本は `DirectBootEpgPending` だけとする。`JobScheduler.getPendingJob()` で同じ固定識別子のジョブが登録済みなら再登録しない。

`BootEpgSyncJobService.onStartJob()` は利用者のロック解除、`DirectBootEpgPending`、開始条件を再確認し、処理を開始する場合は `BootEpgSyncCoordinator` へ引き渡す。`BootEpgSyncCoordinator` は同一プロセス内で `inputId` ごとの起動時 EPG 同期を一度に1件だけ実行する。処理完了時は `jobFinished()` で終了を通知し、成功時は再試行を要求しない。未完了または失敗で `DirectBootEpgPending` が残る場合、または `JobScheduler` による中断で `onStopJob()` が呼ばれた場合は、進行中の走査と Tuner 資源を停止・解放したうえで再試行を要求する。起動時 EPG 同期を開始できなかった場合は `DirectBootEpgPending` を維持する。開始後の保留解除条件は本書「TIS / EPG 公開境界」を正とし、通常publish成功に加えて、開始前の必須TvProvider問い合わせが正常終了し自TIS所有の既存channelが0件だった`NO_WORK`正常終了を含む。

利用者のロック解除までプロセスが生存している場合は、動的に登録した `ACTION_USER_UNLOCKED` の受信処理から同じ開始判定を前倒ししてよい。ただし、この補助経路や定期保守の実行機構だけに再開保証を依存させない。Android の背景実行制限などで起動完了通知が遅延し得ることを前提に、通知の到達時と開始条件の再成立時の双方で永続化した `DirectBootEpgPending` を再評価する。

起動時の EPG 同期と定期保守を開始できるのは、`activeLiveSessionCount == 0`、`sessionCreationInProgress == false`、`setupScanRunning == false`、`playbackPipelineRunning == false`、`scanManager running == false` をすべて満たす場合だけとする。開始条件を満たさない場合は開始を見送る。開始を妨げる状態を更新した後に全開始条件が不成立から成立へ変わった場合は、`DirectBootEpgPending` を再評価し、保留中なら `JobScheduler` に同じ固定識別子の `BootEpgSyncJobService` を登録する判定へ進む。周期的な監視、新しい永続待ち行列、独自の定期実行機構は追加しない。ライブセッション作成要求が来た時点ですでに起動時の EPG 同期または定期保守が実行中なら、当該処理を停止または延期し、ライブ視聴の選局を優先する。

## TIS コールバック入力境界と逆圧

- `SectionEvent.dataLength` は、Tuner コールバックから読み取る section の正確な byte 長として扱う。
- TIS が section event として受け付ける長さは `1..4096` byte だけとする。`dataLength <= 0` は不正、`dataLength > 4096` は過大として、どちらも `ByteArray` 確保前に破棄し、PID別診断に計上する。
- `MediaEvent` sampleは固定4 MiBを上限にしない。負のoffset、0以下のlength、`Int.MAX_VALUE`超過、加算overflow、`offset + length > LinearBlock capacity`はqueue前に拒否する。正常sampleは同一製品profileのper-event予算をclaimし、元`LinearBlock`の有効rangeをcodec別AU解析・再構成・payload copyなしでblock model QueueRequestへ直接渡す。`isPtsPresent=false`はpayload破棄理由にせず、timestampを別eventへ再関連付けしない。共有領域方式とイベント固有fd方式を同じpending byte予算へ計上する。
- decoder／MediaSync入力の逆圧は無通知破棄ではない。sampleまたは未返却audio outputは上限付きpending queueとbudget claimに保持し、後続callback／drainで再試行する。sampleを破棄するのは上限付きqueueが満杯の場合だけとし、破棄counterを加算する。

## provider-data / retry / attribution 境界契約

### Provider-data SSOT

`TvContract.Channels/Programs.COLUMN_INTERNAL_PROVIDER_DATA` の新規書き込みは `arib_si_engine_rs` の provider-data JNI API が返す JSON v1 bytes をそのまま保存する。TIS Kotlin は TvProvider 標準列を詰める接着層であり、provider-data 本体、program stable key、descriptor 診断情報 schema、provider-data digestまたは署名を独自JSON schemaとして再構築してはならない。

Channel provider-data の新規書き込み・読み取り正形式は JSON v1 のみとする。`key=value;...` 形式、旧 flat provider-data、旧 provider-data 断片は読み取り互換入力としても残さない。JSON v1 は `schema="maleicacid.tv.channel"` / `schemaVersion=1` を持ち、channel tune復元に必要な物理選局情報、ONID / TSID / service_id、表示名、放送由来CAS意味事実をRust provider-data API由来の構造として保存する。`inputId`はprovider-dataへ重複保存せず、channel rowのrequired `TvContract.Channels.COLUMN_INPUT_ID`をSSOTとする。`channelRegistrationReady`、`epgPublishable`、`unsupportedCas`、`clearLivePlaybackSupported`等のTIS policyを保存しない。

### 旧 indexed JNI / 廃止経路の禁止

TIS は `nativeSnapshotBulkJson()` と provider-data JNI API を通常境界とする。`nativeGetEventCount()`、`nativeGetEvent*` indexed JNI getter、旧 event JSON `canonicalGenres` フィールド、互換専用の空返却シンボル、未使用 private external 宣言は残してはならない。旧経路を使う呼び出し不能コードや test-only 以外の廃止予定 path は、互換維持ではなく削除する。

### Program publish retry

Program publish retry queue は現行仕様ではprocess-localとする。process death後のretry永続化は行わず、boot/background scanによる再収集を正とする。keyは`ServiceKey + updateWindow`、entryはauthoritative windowと`notBeforeMs`だけを持ち、固定60秒cooldownを適用する。failure classは診断値であってkeyやbackoff入力ではない。queueは単一の有界LRU 512件とし、attempt段階、jitter、retention timer、ServiceKey別上限、retry専用schedulerを持たない。

Provider 必須問い合わせ failure、Program insert/update failure、廃止行削除 failure、publish fingerprint build failureではpublish fingerprint cache更新と `DirectBootEpgPending`解除に進まない。廃止行削除は `deletionAuthoritative=true` の更新区間でのみ実行する。

### AttributionSource

LineageOS 21の通常経路では、`TvInputService.onCreateSession(inputId, sessionId, tvAppAttributionSource)`で受け取ったnon-null `tvAppAttributionSource`をsession寿命中のattribution正本とする。session生成時に`serviceContext.createContext(new ContextParams.Builder().setNextAttributionSource(tvAppAttributionSource).build())`で変更不能なsession固有`sessionContext`を作り、`sessionId`、`tvAppAttributionSource`、`sessionContext`を同じsession creation snapshotへ確定する。途中失敗ではSessionを公開せず、作成済みartifactを解放する。

Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。AudioTrack生成はAndroid 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`を必須とし、`sessionContext.getAttributionSource()`からTV app attribution chainとdevice固有audio session情報を伝播させる。通常経路で素の`serviceContext`をAudioTrackへ渡さず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。生成したAudioTrackは同generationのMediaSyncへ設定し、session releaseまたは置換後は旧`sessionContext`と旧AudioTrackを新しいMediaSync generationへ再利用しない。

`setAttributionSource()`を探索・呼出しするreflection、vendor独自AIDL、reflection失敗時の無言fallbackを通常経路に置かない。例外は本書で明示したMediaSync final-output観測用の製品Framework-private `@hide` contractだけとし、TISを`/system_ext`へ置いて同一platform sourceから型付きcompileする。それ以外のnon-SDK APIを便乗して使用してはならない。


## 本プロダクト対象TSであり得るcodec固定表

ARIB 資料上の本プロダクト対象TSであり得る codec を追加認識対象にする場合は、次の固定表を設計正本に吸収してから扱う。ここでの「扱う」は、PMT / component descriptor / 音声コンポーネントdescriptor / stream type / codecメタデータを認識し、TvProvider / trackメタデータ / 診断情報へ正しく反映することを含む。ISDB-S3 / MMT / TLVは恒久対象外であり、それらだけに依存するcodecまたは音響構成を本表の根拠へ持ち込まない。

### 参照した ARIB 資料と根拠

ARIB適合性の規範対象と検証証拠の分離は `../開発規則.md` を正とする。STD-B32 の規範対象は製品scopeに適用される現行日本語原文4.1であり、レビュー環境の入手可否では変えない。現時点で条項単位に取得して検証証拠として使用できる公式本文は英語版3.11-E1であるため、下表の従来TS profile条項確認には3.11-E1を用いる。ただし4.1日本語原文を本レビュー環境で取得していないため、3.11-E1から4.1までの当該条項差分は未証明であり、下表だけをもって4.1への完全適合確認済みとは扱わない。ARIB公式の4.0/4.1改定概要は、高度地上デジタルテレビジョン放送向けにVVC、MPEG-H 3D Audio、AC-4等が追加・更新されたという適用範囲確認には用いるが、未取得4.x本文の具体条項を推測する根拠にはしない。STD-B79のISDB-T2 / ISDB-T1.5およびSTD-B80のISDB-T3は`開発規則.md`で恒久的な製品scope外とされているため、これら高度地上方式だけに依存するcodecを本product capabilityへ追加しない。

| 根拠資料 | 本改訂で固定する内容 |
|---|---|
| ARIB STD-B32 3.11-E1 Fascicle 1 Chapter 3 3.1〜3.3 | 現行 product が対象とする従来TS profileについて、MPEG-2 Video、MPEG-4 AVC、HEVC の認識根拠として用いる。 |
| ARIB STD-B32 3.11-E1 Fascicle 2 Chapter 3 3.1〜3.4、Chapter 5、Chapter 6 | 現行 product が対象とする従来TS profileについて、MPEG-2 AAC、MPEG-2 BC、MPEG-4 AAC、MPEG-4 ALS の認識根拠として用いる。 |
| ARIB STD-B10 5.13-E1 Part 2 Table 6-5 / 6.2.26 / Annex E | 現行 product が対象とするTS signalingについて、MPEG-2 系映像、H.264/AVC、H.265/HEVC、MPEG-2 Audio、AAC ADTS、MPEG-4 Audio LATM の認識根拠として用いる。 |

### video codec

| codec | 追加認識時の扱い |
|---|---|
| MPEG-2 Video | 必須対応。PMT / component descriptor から codec、解像度、走査方式、aspect を認識し、MediaFormat、block model decoder起動、MediaSync first-frame gate、unsupported診断情報を固定する。 |
| H.264 / MPEG-4 AVC | 必須対応。profile / level は AVC video descriptor と実 MediaCodec capability を照合し、未対応時は codec unsupported 診断に落とす。 |
| H.265 / HEVC | codec として認識対象。r51はmetadata / 診断へ保持する。r52では現行対象の従来TS profileでARIB signaling上HEVCが現れる場合をgeneric direct playback selectionへ含め、MediaFormat / block model decoder / MediaSync first-output gate / unsupported診断まで必須とする。 |

ISO/IEC 14496-2 Visual、JPEG 2000、auxiliary video、SVC、MVC、3D additional view は、今回の ISDB-T/S product scope のライブviewable codecとして対応宣言しない。必要ならprovider-data / 診断情報にARIB signalingを保持する。

### audio codec

| codec | 追加認識時の扱い |
|---|---|
| MPEG-2 AAC | 必須対応。ADTS / MPEG-2 AAC LC、channel count、sample rate、ISO639 language、main/sub、dual mono、音声モード、音質表示を保持する。 |
| MPEG-2 BC Audio | 認識対象。decoder が利用できる場合だけ再生対応を対応宣言し、未対応時はvideo-only診断に落とす。 |
| MPEG-4 AAC / HE-AAC | 必須認識。AAC LC / HE-AAC profile、LATM/LOAS / ADTS、AudioSpecificConfig、channel count、sample rate を保持する。decoder が利用できる場合だけ再生対応を対応宣言する。 |
| MPEG-4 ALS | codec として認識対象。対象 transport profile を本プロダクトが対応宣言しない場合はplayable capabilityに入れない。対応する場合は block model decoder / MediaSync / AudioTrack / メタデータ / unsupported診断情報まで必須。 |

MPEG-H 3D Audio と AC-4 は ARIB STD-B32 4.0以降の改定概要で高度地上デジタルテレビジョン放送向け追加codecであることを確認できるが、STD-B79 / STD-B80 の高度地上方式は本productの恒久scope外であるためcodec固定表には含めない。AC-3、Enhanced AC-3、DTS、DTS-HD、Dolby TrueHDも現行対象transportに対する取得可能なARIB本文の条項根拠を確認せず推測で追加しない。

## provider-data 生成・受け渡し境界

TIS は TvProvider 標準列への投影を担当する。TIS は `Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` に保存される最終 JSON を直接生成してはならない。

Program provider-dataはRustが同じbulk SI transactionのEIT event / service / `ServiceSemanticFacts` / descriptor診断から直接生成し、`AribEvent.providerDataCanonicalJson`として返す。TISは標準列用の構造化event fieldから`JSONObject` requestを再構築せず、このcanonical JSONだけを`ProgramRecord`へ透過保持して保存する。

Channel provider-dataはfrequency、delivery system、stream selector等のTIS-owned tune identityを必要とするため、`maleicacid.tv.channelRequest`をRustへ渡す。この受け渡し用形式は保存用`maleicacid.tv.channel`を名乗らず、Rustのclosed serde型を正とする。TISはこの受け渡し用JSONをprovider-data schemaのKotlin実装、保存形式または正規形として扱ってはならない。

RustはProgramをtyped意味stateから、Channelを検査済みrequestから生成し、保存用JSON、識別子、切り詰め診断を確定する。TIS runtimeのpolicy結果、track identity、Android投影結果は保存JSONへ戻さない。

TIS は保存データの型、正規化、必須項目判定、欠落補完、旧形式互換、識別子抽出、サイズ上限処理を実装してはならない。TIS 側で `0`、`false`、`jpn`、`UNKNOWN`、空文字などを使って必須項目欠落を補い、provider-data を成立させてはならない。

`DescriptorDiagnosticV1` はRustが解析したdescriptor診断modelからProgram provider-dataへ直接格納する。TIS は `DescriptorDiagnosticV1` を項目ごとに再構築してRustへ戻してはならない。TIS が保持する場合は、Rust 生成の正規 JSON を不透明な文字列として透過保持する。

TIS の試験は、Channel受け渡し用 JSON の細部を保存形式として検査しない。ProgramはRust bulk snapshotが返す保存用JSON、ChannelはRust builderが返す保存用JSON、識別子、拒否診断を検査する。
