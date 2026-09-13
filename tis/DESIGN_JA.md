# TIS 設計判断

## AOSP 標準経路

TIS は `TvInputService` としてシステムTVアプリから呼ばれ、Tuner HAL には Tuner SDK API 経由でアクセスする。HAL binder を直接呼ばない。
TIS の setup / boot EPG sync / user unlock drain は、固定文字列や package 名を inputId とみなしてはならない。`TvInputManager.tvInputList` から自 `MaleicacidTvInputService` に一致する `TvInputInfo.id` を一意に解決し、その inputId だけを scan / sync / TvProvider writer へ渡す。解決不能または複数一致の場合、boot EPG sync は pending のまま延期し、setup scan は開始しない。

### SI収集の期限と失敗境界

走査の待機時間・安定待ち・最大期限は`SystemClock.elapsedRealtime()`の差で測り、端末の時刻補正に依存させない。SI境界のcollectionは`../arib_si_engine_rs/DESIGN_JA.md`の有限寿命・入力上限に従う。上限時に破棄されたsnapshotから登録完了・EPG完了を導出しない。継続視聴のdecoder資源寿命はSI collectionの再同期とは独立している。

## BS と CS110 の選局契約

BSはIF周波数とAOSP Tuner公開契約のtyped stream selectorを保持する。通常のscan候補、channel保存、再選局ではbackend種別に依存せず、`STREAM_ID`のTSID `0..65534`だけを使用する。TISはpx4の相対slot、Linux DVBの`DTV_STREAM_ID`、HAL内部のbackend種別・能力を取得・推測・保存しない。一方、AOSP Tuner SDKが公開する`FrontendInfo.type`と`statusCapabilities`はfrontend選択とBS候補source選択に使用する。CS110は周波数帯だけでscan candidateとtune selectorを作り、stream selectorを保存しない。

CS110のTIS内部モデルとTvProvider保存形式では、frontend stream selectorを`None`／`null`として保持する。Android 15 Tuner API builderへ変換するときは`streamId`と`streamIdType`のsetterをどちらも呼ばない。builderが生成する`STREAM_ID`と`INVALID_STREAM_ID(0xFFFF)`の組を、Tuner HALが公開契約境界で`NoSelector`へ正規化する。TISから`UNDEFINED`、0、TSID、relative番号を「selectorなし」の代用として明示設定しない。CS110 の ONID / TSID / service_id は channel identity / サービス識別子として保持してよいが、HAL frontend selectorへ転用してはならない。BSの通常製品経路はIF周波数と`STREAM_ID`のTSIDを使う。TISはdriver固有slotへ変換せず、typed selectorの検証とbackend ABIへの写像はTuner HALへ委ねる。

TvProvider の channel internal provider data には JSON v1 `tune.streamIdType` と `tune.streamId` を保存する。通常製品経路で書き込む値は、`NONE` の `streamId=null`、または `TSID` の `0..65534`だけとする。`65535`はAOSP `INVALID_STREAM_ID`であり、実TSIDとして保存または再投入しない。`RELATIVE`はAOSP Tuner AIDLで合法なtune-time selector種別だが、本製品では永続channel tune identityとして採用しないため、TISの通常channelデータへ保存しない。


## 製品 scan 候補表の保持者

製品scanの選局対象、周波数帯、CATV中心周波数、VHF除外、BS/CS110 selector境界を含む規範値は、tv直下の`開発規則.md`の「製品 scan 候補の規範値」を唯一の設計正本とする。

TISの物理候補表は製品scan実装データのSSOTであり、`開発規則.md`の規範値に従うRF候補と、dynamic discovery非対応frontend用の固定RF→absolute TSID候補を保持する。BS setup/rescanの候補source選択は`開発規則.md`の「製品 scan 候補の規範値」を正とする。TISは`Tuner.getAvailableFrontendInfos()`でISDB-S frontendを列挙し、`FrontendInfo.statusCapabilities`の`FRONTEND_STATUS_TYPE_STREAM_IDS`を持つ候補を優先して`Tuner.applyFrontend()`で確保する。確保したfrontendが同capabilityを持つ場合は物理RFごとにstream selector未指定の`IsdbsFrontendSettings`で`Tuner.scan()`を実行し、`ScanCallback.onInputStreamIdsReported()`で得たcurrent stream IDをtyped `STREAM_ID` explicit tune candidateへ変換する。 選択したfrontendはBS setup/rescanの候補source lifetime中保持し、RFごとの`cancelScanning()`後に`closeFrontend()`しない。explicit tuneと次RFのdynamic scanは同じ選択frontendを継続使用する。同capability frontendを確保できず、非対応frontendを確保できた場合は固定RF→absolute TSID候補を使う。dynamic scan開始後の失敗、timeout、空報告から固定候補へ切り替えない。候補を実際にtuneした後、PAT/NIT/SDT actualからONID/TSID/SIDとcurrent transportを確認できたserviceだけを登録・公開する。driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。

## サービス登録・公開・再生policy境界

`arib_si_engine_rs` が返すservice / transport単位の `ServiceSemanticFacts` をAndroid channel登録、EPG公開、ライブ再生へ接続する判断はTISが所有する。`ServiceSemanticFacts` はONID / TSID / SID、ARIB `service_type`、PMT/PCRの存在・構文状態、ES/component一覧とcodec signaling、CA descriptor / free_CA_mode、CA descriptor等から導出した`requiresCas`、SMD意味状態、欠落・不正理由など放送由来の事実だけを含む。`channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackSupported`、`unsupportedCas`のような現在の製品能力・TIF policy結果は含まない。

TISはcurrent `ServiceSemanticFacts`から`requiresCas`を意味事実として受け取り、SI段階では現在releaseの対応service type/codecとCAS状態から `channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackStaticallyEligible`、`unsupportedCas` を算出する。`clearLivePlaybackStaticallyEligible` はdecoderを開く前の静的候補factであり、`clearLivePlaybackSupported`を表明しない。実decoder availabilityはlive playback開始時のMediaCodec選択・configure成功を正本とし、static eligibilityを満たしてもdecoderを利用できないserviceは再生成功扱いにしない。このpolicy結果はSI parserへ逆流させず、保存済みprovider-dataをcurrent policyのfallback sourceにしない。

SMDの通常受信対象判定では、`arib_si_engine_rs`が返す`broadcasting_flag` / `broadcasting_identifier`の放送由来事実と、TISが現在処理している`ScanCandidate.kind`を組み合わせる。`ISDB_T_UHF` / `ISDB_T_CATV`は地上デジタルテレビ`0b000011`、`ISDB_S_BS`はBSデジタル`0b000010`、`ISDB_S_110CS`は広帯域CSデジタル`0b000100`を期待値とする。`broadcasting_flag=0b00`の正常SMDでもcandidateと`broadcasting_identifier`が一致しないserviceは、そのcandidateでは`channelRegistrationReady` / `epgPublishable` / `clearLivePlaybackStaticallyEligible`にしない。ONIDから放送方式またはSMD期待値を推定せず、ONIDはnetwork/service identityとしてのみ用いる。

partial snapshot はサービス単位の登録可能判定に使ってよい。ただし partial snapshot を無条件に channel 登録へ出してはならない。登録可能サービスは、ONID / TSID / SID、PMT PID と PMT、有効 PCR、後続更新可能な internal key、および現行ライブ視聴で対応するaudioまたはvideo ESを持つサービスとする。video-only / audio-onlyというtrack構成は`TvContract.Channels.COLUMN_SERVICE_TYPE`の再分類根拠にせず、同列は`../ARIB_SI_EPG_TvProvider投影方針.md`に従ってARIB `service_type`のcodingを保持する。audio-onlyの視聴セッションでは`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`を通知できるが、この値をchannel登録の禁止理由に使わない。音声・映像の欠落または未対応はTIS側のtrack別診断に残す。scrambled サービスはTIS policyでchannel登録してよいが、現行の平文ライブ視聴成功対応宣言対象にはしない。登録可能未満の partial snapshot は診断情報 / ライブ更新 / debugに限定し、channel insert に使わない。

`TvTrackInfo` の `trackId` はAndroid/TIS runtimeの識別子であり、TISがcurrent serviceのcomponent identityからcurrent session内で一意になるよう決定する。ARIB意味objectや永続`internal_provider_data`に`trackId`を保存せず、Rust SI parserへ返さない。

Audio track metadata はPMTとEITの責務を混同しない。実際にfilter/decoderへ渡すPIDと`stream_type`はcurrent PMT ESを正とし、current EIT `audio_component_descriptor`は同一`component_tag`のESに対する放送意味metadataとして相関する。ARIB STD-B10の`component_tag`はPMTのstream identifier descriptorの同fieldと同値であるため、この相関以外のPID順序やtrack順序による推測を行わない。descriptorが欠落・不正・`component_tag`不一致の場合はPMTで確定できるtrack identity/codec情報だけを通知し、EIT metadataを捏造しない。

有効な`audio_component_descriptor`からAndroid `TvTrackInfo`へ自然対応する値だけを投影する。ISO 639 languageは`setLanguage()`、PMT `stream_type`から一意に決まるMIMEは`setEncoding()`、ARIB `sampling_rate`の明示値は`setAudioSampleRate()`、`component_type`下位5bitのaudio modeから一意に決まるchannel数は`setAudioChannelCount()`、`text_char`は`setDescription()`へ投影する。`component_type` b6-b5=`01`の視覚障害者向け音声解説は`setAudioDescription(true)`、`10`の聴覚障害者向け音声は`setHardOfHearing(true)`へ投影する。reserved/未定義audio mode、reserved sampling rate、PMTとdescriptorで意味が一致しない値を推測で標準fieldへ埋めない。`main_component_flag`、`ES_multi_lingual_flag`、`quality_indicator`、`simulcast_group_tag`等は放送由来factとして保持するが、意味の異なるAndroid標準fieldへ転用しない。dual monoの主/副/主副presentationは1 ESのpresentation stateとして`AudioTrack.setDualMonoMode()`へ接続し、別audio trackを捏造しない。

AudioTrackへ渡すPCM topologyはdecoderの実出力を正本にする。decoder output `MediaFormat`に有効な`KEY_CHANNEL_MASK`がある場合はその`AudioFormat` positional maskを最優先で使用し、入力header、PMT、EITから別maskを上書き生成しない。`KEY_CHANNEL_MASK`が無い場合だけ、同一trackの有効なARIB `audio_component_descriptor.component_type`からspeaker topologyを一意に導出でき、かつdecoder outputの`KEY_CHANNEL_COUNT`と一致する場合にそのtopologyを使用する。それも確定できない場合はAOSPの`audio_channel_out_mask_from_count()`相当のcount→canonical maskをfallbackとして使用する。このcount fallbackはcontent channel mask不明時の便宜的配置であり、3ch等のARIB固有topologyを正確に復元した事実として扱わない。Android 13以降のdefault AAC decoderが3ch以上を出力するにもかかわらずoutput `KEY_CHANNEL_MASK`を提示しない場合は、fallbackで再生継続できてもCDD §5.1.2 C-7-2の適合を満たしたとは扱わず診断する。AudioTrackはdecoderのcurrent output formatに対して構成し、sample rate / channel count / channel maskが変わるoutput-format changeでは旧AudioTrackをそのまま使わず、既存のplayback generation / MediaSync再生成契約に従って新しい実出力形式へ同期する。

Video track metadataもPMTとEITの責務を混同しない。filter/decoderへ渡すPIDと`stream_type`はcurrent PMT ESを正とし、current EIT `component_descriptor`は同一`component_tag`のvideo ESへだけ相関する。有効descriptorの`text_char`は`TvTrackInfo.setDescription()`へ投影してよいが、ARIBの`component_type`から得る480i/720p/1080i、aspect、scan等の放送形式factを、対応するexact Android標準fieldがない場合に別意味へ変換しない。特に`component_type`の「1080i」等だけから`1920x1080`等のpixel width/heightを推測して`setVideoWidth()` / `setVideoHeight()`へ設定しない。exact pixel geometryはdecoder output `MediaFormat`またはcodec headerから実値として確定したwidth/heightを正本とし、EIT descriptor factはprovider-data / diagnostic側で保持する。

## 録画・予約の現行除外

現行 product では録画・予約を製品機能として表明しない。TIS メタデータの `android:canRecord` は `false` のまま維持し、`MaleicacidTvInputService.onCreateRecordingSession()` は `null` を返す。`RecordingSession`、DVR/file output、`RecordedPrograms` 登録、`notifyRecordingStopped()` / `notifyError()`、`TvRecordingClient` による予約録画開始は現行 product 対象外である。

`rec/` 配下の実装とテストは録画・予約作業用の準備領域であり、現行 product package、TIS manifest、boot receiver、release確認条件へ混ぜない。現行 product で起動してよい receiver / サービスは TIS のライブ視聴・setup・EPG publish に必要なものだけとする。

## CAS / descrambler 境界

TIS は Tuner SDK API の filter 経由で PMT/CAT/SDT/ECM/EMM section payload を取得し、PMT/CAT から得た CA_descriptor と SDT 等から得た free_CA_mode / サービス識別子補助情報を `arib_si_engine_rs` の意味解析結果として受け取る。CA system IDはB25 `0x0005`とB1 `0x0001`を区別し、current `ServiceSemanticFacts`とcurrent MediaCas capabilityからsession/filter構成を決める。CAS HALのpath選択、card I/O、ECM/EMM意味処理、鍵registryは `../cas_hal/DESIGN_JA.md` を正とし、TISへ複製しない。

B25ではPMT/CAT由来のECM/EMM filterを開き、`MediaCas.Session.processEcm()`と`MediaCas.processEmm()`を呼ぶ。B1ではPMT由来のECM filterだけを開き、CAT由来のB1 EMM metadataをfilter起動対象にせず、`MediaCas.processEmm()`を呼ばない。CA system IDをまとめた共通集合だけでEMM可否を決めてはならない。B1 EMM、通電制御情報取得、契約更新、権利更新をTIS側で補完しない。

ECM成功後にTunerへ渡すproduction tokenは、同じ `MediaCas.Session.getSessionId()` が返すbyte sequenceそのものである。`processEcm()`の戻り値からvendor tokenを得る、診断文字列をtoken化する、CA system ID/session generation/key epochをTISで符号化する、raw key materialを受け取る経路は設けない。session IDは1 byte以上16 byte以下で `[0x00]` ではないことを型付き境界で確認し、同じbytesを変更せず `Descrambler.setKeyToken()`へ渡す。ECM失敗、session ID不正、CAS registry未接続、診断専用結果では`setKeyToken()`を呼ばない。

TISはdescramblerを同じTuner/demux generationに結合し、token成功後に当該CA descriptorが対象とするvideo/audio PIDだけを`addPid()`する。service/track更新では不要PIDを`removePid()`し、session close、retune、clear service遷移、CAS failureではdescramblerとMediaCas sessionを世代付きで閉じる。stale ECM/EMM callbackや旧session IDを新generationへ適用しない。Tuner HALが未接続、bad token、registry failure、descramble failureを返した場合も平文成功扱いにしない。

descrambler bridgeの単一所有者はCasControllerとし、TunerControllerに同じbridgeをcacheしない。scan/liveは取得済みbridgeを持ち回らず、受信snapshotに対応するtune generationを必須入力とするTunerController.updateCasMetadataAndFilters()を使う。同controller executor内でtuneAcceptedとgenerationを照合し、CasControllerのfactoryによるbridge生成・attach・metadata更新と、成功結果のECM/EMM PIDを使うfilter更新完了まで待つ。metadata成功前に新filter集合を公開しない。resource-lostは同executorで直列化するため、取得とattachの間に割り込ませない。旧世代・失効済み要求はfactoryを呼ばず拒否する。

metadata更新では配送indexを先に失効させ、全obsolete contextと不要pluginを物理解放前に退役させる。解放は全件試行し、survivorのprivateData・PID bindingとdescrambler PID更新が全て成功した場合だけ配送indexを公開する。途中失敗から旧bindingを再公開しない。metadata診断error・例外・filter更新失敗は同じcontroller transactionでCAS解放、ECM/EMM filterの空集合への置換、再生停止を全件試行する。PMTは判断更新用に維持し、caption終了と利用不能通知はLive sessionが行う。

resource-lostと再選局時はCasController.clearForResourceLoss()がCAS state清掃とbridge closeを全件試行する。close成功後だけ所有参照を落とし、失敗中はclosingとして保持してECM/EMMを配送しない。終了するcontextのPID/key linkの解放はbridgeのcloseに集約し、その直前のremovePid列挙やVOID token設定を要求しない。未生成bridgeはPID/key linkを持たず、cleanupのためにhandleを遅延生成しない。CAS session/pluginも退役時に配送対象から外し、各closeの成功を記録して未解放資源だけを再試行する。次のbridge生成前またはclose()で退役資源の解放を再試行し、成功するまで新bridgeを生成・attachしない。失敗時に独立したretry queueや新しい世代は作らない。DirectTunerDescramblerBridge.close()は未生成handleを生成せず、実close失敗を伝播し、閉鎖開始後のsetKeyToken/addPid/removePidと再利用を拒否する。CAS全体のclose失敗でもexecutorと所有を残してclose再試行を可能にする。

MediaCas Session/Plugin adapterは公開close()が返す例外をそのままCasControllerへ伝播する。同じpluginの全Sessionについてclose成功を確認してから親のMediaCasを閉じる。AndroidのMediaCas.close()は親の内部参照を無効化し、以後のSession.close()を拒否するため、Session解放失敗中に親を閉じて再試行経路を失わせない。別systemのSession/pluginの解放は引き続き全件試行する。CAT由来EMMだけのsystemは既存system台帳にpluginのみを所有し、processEmmにSession生成を要求しない。同systemにESへ展開済みのECM bindingが現れた時点で同じpluginからcontext単位のSessionを開く。PROGRAM bindingだけで対象ESが未確定の間はREADYにしない。ECMがなくなりCATだけ残る場合はSessionだけを閉じる。openSession失敗時も同じ台帳で退役・解放する。rollback close失敗は元のsession-open失敗へ添え、成功するまでpluginのownerを除去しない。Framework内部で握り潰され公開APIへ返らない失敗までTISが検出できるとは扱わない。

ES PIDの所有は独立contextごとのDescramblerで管理し、別contextが同じPIDを使用していてもそのbindingとは混同しない。継続するcontextではdesired/linked PID集合の差をaddPid/removePidで更新し、成功した操作だけを反映する。非SUCCESSはCAS診断failureとして保持し、次のmetadata更新で未完了差分を再試行する。context終了時はDescrambler.close成功でPID/key所有をまとめて解消し、個別unlinkの再試行台帳を追加しない。初回key linkの部分成功を取り消す場合のremovePid/VOID key unlinkは維持する。

### context・filter plan・readiness

TISはMediaCas / MediaCas.SessionとTuner SDK Descramblerだけを使い、CAS/Tuner HAL binderやgeneric key provisioning socketを直接呼ばない。Tuner HALもCA system、ECM/EMM、MediaCas sessionの意味を解釈しない。

独立descramble contextはservice identity、CA system ID、ECM PID、CA descriptor private dataと適用scopeに基づいて決め、一つのMediaCas.Session、そのopaque session ID/token、一つのDescrambler/key slot、その鍵で保護される一つ以上のES PIDを所有する。同じsystemのMediaCas pluginは共有してよいが、異なるECM/private dataや独立key contextを一つのSessionへ押し込まない。別bindingのsetPrivateDataで同じSessionを上書きして兼用しない。「1 CA system = 1 Session/Descrambler」「1 ES = 1 Descrambler」を固定規則にしない。

Program-level descriptorは適用されるESへ展開してから同一service/system/ECM/private dataで束ね、別ECM/private dataを持つES-level descriptorは別contextにする。B25/B1固有のfilter可否はCasControllerが決め、UpdateResult.ecmPids / emmPidsを唯一のCAS filter planとする。LiveSessionはraw metadataからPID集合を再構成しない。B1 CAT metadataはSI事実として保持できるが、EMM filter/process対象にしない。

readinessはCLEAR（context不要）、WAITING_FOR_KEY（contextは成立したがECM/key/PID link未完了）、READY（必要な全contextが準備済み）、ERROR（blocking failure）、CLOSED（終了済み）を区別する。WAITINGをclear playback成功へ写像しない。LiveSessionはsection ingest後にcurrent-stateを再評価し、READYで通常のmaybeStartPlayback gateへ進む。通常の後続ECM/CW rotationは同じkey slotを更新するため、link済みcontextのAV pipelineをECMごとに再生成しない。

### 終了順序とexecutor寿命

retune、clear service、context消滅、CAS fatal failure、session releaseでは対象contextをcurrent routingから先に退役させ、次の順で解放を試みる。

1. Descrambler.close。PID/key linkの個別解除は重ねない。
2. MediaCas.Session.close。
3. 同じsystemの全Sessionが解放済みで、Session/EMM処理が不要になった場合だけMediaCas.close。

途中の失敗で独立した後続解放を中止しない。contextのDescrambler/Sessionは各close成功を記録し、未解放資源だけを再試行する。metadata更新は退役資源の解放完了まで新routingを公開しない。TISはvendor registryへRevokeを送らず、Session closeに伴うprovider-side revokeはCAS HAL側に任せる。

mutable状態の直列化境界はCasController専用single-thread executorである。内部executor判定はthread名ではなく生成したThread instanceのidentityで行う。closeは退役、解放、CLOSED診断確定を直列化し、全解放成功後だけexecutorをshutdownする。失敗中は所有とexecutorを保持する。closeは冪等で、終了後のmutationは拒否し、lastDiagnostic/currentReadinessは停止済みexecutorへworkを投入せずCLOSEDを返す。

context/Descrambler factoryはcurrent Tuner/demux generationのCAS更新transactionに属する。retune/clear/release後の旧callback・旧tokenを新contextへ流用せず、READY/ERRORはcurrent context集合だけから判定する。

### CAS orchestrationの最低試験

- 同一system・同一ECM/private dataの複数PIDは同じcontext/Descramblerを共有し、異なるECMまたはprivate dataは独立する。
- 一方のcontextのsetKeyTokenが他contextのkey linkを置換しない。
- B1 CATからEMM filter planを作らず、scrambled serviceはECM前にWAITING、必要contextの成功後だけREADYになる。
- 終了時は個別PID/key unlinkなしでDescrambler、Session、pluginの順に閉じ、close失敗時も後続解放を試行して未解放資源だけ再試行する。
- closeを2回呼べ、終了後queryはCLOSED、新規mutationは拒否される。未生成Descramblerをcleanupで生成しない。
- retune/clear後のstale ECMが旧token/PIDを新generationへ適用しない。継続contextのPID差分と初回link失敗rollbackは個別操作の成功・失敗を引き続き検証する。

## Tuner SDK API 呼び出し

`openDescrambler()`、`setKeyToken()`、`addPid()`、`removePid()` は reflection を使わず、対象 build の system/privileged API として直接呼ぶ。本製品buildはこれらのAPIを提供するplatformと一体で構成することを恒久的なintegration prerequisiteとし、欠くbuildを本製品構成として成立させない。runtimeでreflection、代替API、HAL binder直呼びへfallbackしてこの前提を回避しない。

## 再生経路

製品ライブAVのアーキテクチャは、`../開発規則.md` のproduct-level invariantどおり **clear-memory / non-passthrough** のAOSP Tuner標準経路に固定する。TISはTuner media filterが返す各`MediaEvent`について`getLinearBlock()`と`getOffset()` / `getDataLength()`で示された有効rangeを、そのまま`MediaCodec.CONFIGURE_FLAG_USE_BLOCK_MODEL`の`QueueRequest.setLinearBlock()`へ渡す。AOSP Tuner Frameworkが同じnon-passthrough media filterについてデータ形式をESまたはpartial ESとしつつ`MediaEvent.getLinearBlock()`を直接MediaCodecへqueueする利用フローを規定しているため、TISは`MediaEvent`境界の上にcodec別access-unit parser、AU再構成、PES再解析を追加しない。

TISはMPEG-2 Video / H.264 / HEVC / AAC / MPEG audioのstart code、NAL、slice、ADTS frame、MPEG audio frameを通常入力経路で再解析してqueue境界を作らない。`MediaEvent`がpartial ESを含み得ることはTuner→MediaCodec境界の契約として受け入れ、`BUFFER_FLAG_PARTIAL_FRAME`の手動付与、TIS所有`LinearBlock`への再構成、ES全体またはAU単位のcopyを標準経路にしない。対象decoder/device profileがこのAOSP direct-input契約を満たさない場合は、その組合せをplayback capability qualificationで非対応にする。runtimeでcodec parser/reassemblerへfallbackしてAOSPの責務境界を複製しない。

各`MediaEvent`のtimestamp metadataもevent単位で透過的に扱う。`MediaEvent.getOffset()`をPTSの適用位置またはPES header位置とは解釈しない。AOSP契約上、`isPtsPresent()`は元PES headerに明示PTSが存在したかというprovenanceを表し、`getPts()`はaudio/video frameの90 kHz presentation timestampを表す別フィールドである。本製品が成功対応として表明するclear / non-passthrough live media-filter profileでは、Tuner HAL / media-filter producerがすべてのnon-empty `MediaEvent`について、当該eventのESデータへ適用可能な有効な33-bit 90 kHz presentation timestampを`getPts()`で提供することをproducer/consumer契約とする。明示PTSの適用先、後続audio AUの時刻、前PESから継続するAUの時刻とprovenanceは、`../tuner_hal/DESIGN_JA.md` の「clear non-passthrough MediaEvent presentation timestamp 契約」を正とする。明示PTSはaudio PES内で最初に開始するAUに対応し、同じPES由来の全eventへ同じPTSを要求しない。PTSを明示しない合法なPES由来eventでは`isPtsPresent()==false`を維持し、hardware demux / driver / backend media extractor等のproducer側が当該eventのESデータに対応するpresentation timestampをauthoritative timing metadataとして既に確定できる場合に限り、その値を`getPts()`へ設定する。HAL共通層は定数0、単純な直前PTS carry-forward、PCR、wallclock、nominal frame rate、sample rate等からpresentation timestampを推測生成しない。producer側境界でも当該eventとのauthoritative associationを確定できないbackend/profileは、このlive direct-input成功対応profileとして表明しない。`isPtsPresent`をtimestamp validity flagへ読み替えず、provenanceを偽装して`true`へ丸めない。

TISは`isPtsPresent()`をMediaCodecへqueueする／しない、drop、playback fatalの判定に使用しない。non-empty `MediaEvent`ではproducer-authoritativeな`getPts()`を33-bit range検証し、`PtsNormalizer`へ渡して同じeventの`QueueRequest.setPresentationTimeUs()`へ必ず設定してからdirect queueする。Android 15の`MediaCodec.QueueRequest`はpresentation timestampのabsenceを表現できずsetter未呼出しでは0がqueueされるため、setter未呼出しを「timestampなし」として利用しない。TISは0、直前PTS、PCR、wallclock、frame rate、sample rateからtimestampを補完せず、別eventやcodec AUへPTSを再関連付けせず、codec別AU parser、PES再解析、AU再構成も追加しない。producerが上記保証を満たせないbackend/profileはlive direct-input成功対応profileとしてqualificationを通さず、成功capabilityとして表明しない。これは公開Tuner AIDL/VINTF/VTSのフィールド意味を変更するものではなく、既存`MediaEvent.pts`を使って製品内producer/consumer責務を閉じる追加契約である。

最低試験は、(1) explicit PTS video PESとaudio PES内で最初に開始するAUでは`getPts()`がその明示PTSとなり、後続AUと前PESから継続するAUではHAL正本のAU対応規則による値をTISが変更せずqueueすること、(2) 合法なPTS-sparse inputで`isPtsPresent()==false`でもbackendが当該media outputに対応するauthoritative timing metadataを持つ場合はその対応値を`getPts()`へ出し、TISがdrop/fatalせずqueue継続すること、(3) authoritative sourceがない場合にproducer共通層／TISのどちらも0、直前PTS、PCR、wallclock、frame rate、sample rate等からtimestampを推測生成せず、そのbackend/profileをlive direct-input成功capabilityとして表明しないこと、(4) 33-bit wrap前後とA/V間で本来のtimeline差を維持すること、(5) TISが`isPtsPresent()==false`だけを理由にdrop/fatalしないこと、を含める。

`PlaybackPipeline` はplayback generationごとに1個の`PtsEpochCoordinator`と、active compressed trackごとの`PtsNormalizer`を持つ。これはcodec framingやAU identityを所有せず、**producer contractで有効な`getPts()`を持つ全non-empty `MediaEvent`のtimestamp変換だけ**を担当する。`PtsNormalizer`の`rawPrev` / `extendedPrev`はtrack別とし、33-bit wrap epochだけをgeneration内で共有する。`M = 2^33`、`H = 2^32`、`signedDelta(rawNew, rawRef) = ((rawNew - rawRef + H) mod M) - H`を`[-H, H-1]`の差とし、半周期差は`-H`に固定する。generationで最初のproducer-authoritative `getPts()`を共通extended seed `H`へ置き、後から開始または置換されるtrackはそのtrackで最初のproducer-authoritative `getPts()`をcurrent coordinator referenceに対してsigned-moduloで同じepochへjoinする。seed済みtrackはtrack-localにunwrapする。`isPtsPresent()==false`でも`getPts()`は通常どおりcoordinator / normalizerへ入力し、provenance bit自体はunwrap状態やqueue可否を変更しない。PTS deltaの大小、通常wrap、presentation-order reorderだけから独自discontinuityを推定しない。

plain `Filter.flush()`はAOSP契約どおりfilterが生成済みで未消費のdataをclearする入力側操作とし、未queue `MediaEvent` / `LinearBlock`と対応claimだけを破棄する。plain flushだけでは`PtsEpochCoordinator`、seed済み`PtsNormalizer`、decoder、MediaSync、AudioTrack、playback generationをresetしない。decoder再生成、AudioTrack切替 / 再生成、audio route変更、Surface変更、またはcodec / PID / track graph変更を伴うretune・track切替は後段のlifecycle契約どおりfull playback generation resetとし、coordinatorと全normalizerを新seedから開始する。filterのstop/reconfigure/restartに伴う`RestartEvent`は旧configuration eventを捨てるevent-validity境界であり、playback graphが変わらない限りそれ単独ではgeneration resetにせず、任意のPTS jump検出器としても使わない。

`MediaEvent.getOffset()` / `getDataLength()`は`long`のまま境界検査し、`offsetLong >= 0`、`dataLengthLong > 0`、`offsetLong <= Int.MAX_VALUE`、`dataLengthLong <= Int.MAX_VALUE`を先に満たすことを要求する。次に`offsetLong + dataLengthLong`をchecked additionで算出して`long` overflowを拒否し、`LinearBlock`のqueue可能range / capacity以下であることを確認する。すべて満たした後だけ`Math.toIntExact()`相当でoffset / sizeを`int`へnarrowし、`QueueRequest.setLinearBlock()`へ渡す。implicit cast、truncate、narrow後のoverflow検査は禁止する。`getLinearBlock()`がnull、block model configureまたはQueueRequestが利用不能、range / narrowing検査違反、decoderがAOSP direct-input契約を満たさない場合は成功を偽装せず型付き診断へ落とし、当該playback profileを成功対応として表明しない。

source `MediaEvent` / `LinearBlock`とrangeに対応するbudget claimは、`QueueRequest.queue()`が成功してcodecへ所有権を移すか、当該eventの破棄が確定するまで保持する。TISは通常ES payloadを`LinearBlock.map()`から`ByteArray`、別`LinearBlock`、通常ByteBuffer input modelへ複製しない。header / MediaFormat確定のために必要な最小prefixをread-only参照することは許すが、それをAU parserまたはES搬送経路へ拡張しない。

secure-memory handle、tunneled playback、platform passthroughは本製品が提供しない恒久的なplayback capabilityであり、時点依存の一時除外ではない。TISはsecure `MediaEvent`をclear-memoryへcopyできると仮定せず、その経路をadvertiseしない。r52のCAS対応もdescramble後のclear ESをこのdirect non-passthrough経路へ接続できるサービスだけをライブ視聴成功対象とする。

デコード後のA/V同期とSurface提示はAndroid標準`MediaSync`だけを使用する。decoder output の `BufferInfo.presentationTimeUs` をMediaSyncへ渡すmedia timeの正本とする。video decoder出力は`MediaSync.setSurface(sessionSurface)`後の`createInputSurface()`へ接続し、output timestampをMediaSyncへ渡す。audio decoder出力PCMは`MediaSync.queueAudio()`へpresentation time付きで渡し、`MediaSync.Callback.onAudioBufferConsumed()`を受けるまでaudio output bufferの所有権を保持する。独自media clock、PCR→wallclock変換、独自future render / late drop schedulerを設けない。

### MediaSync Framework-private final-output observation

stock Android 15 / LineageOS 22.1 の `MediaSync` はvideo scheduling/dropをnative側で所有し、late frameをinputへ返すdrop分岐と、render対象frameをcurrent outputへattachして`queueBuffer()`する分岐を区別する。一方、公開Java APIにはそのfinal-output成功をvideo clientへ通知するcallbackがない。この不足だけを閉じるため、対象LineageOS platformの`android.media.MediaSync`へ、既存public `MediaSync.Callback`とは別の `@hide OnFirstVideoFrameQueuedToOutputListener` と、arm識別子を同時に設定する `@hide setOnFirstVideoFrameQueuedToOutputListener(long armSequence, listener, handler)` 相当を追加する。Framework側は`armSequence`をTIF/TIS固有の意味を解釈しないopaque値として保持し、listener eventは少なくとも`MediaSync` instanceと成功判定時に固定した`armSequence`を返す。public SDK、`@SystemApi`、`@TestApi`、Tuner AIDL/VINTFは変更しない。

availabilityをarmするたびに、`PlaybackPipeline`はimmutableな`AvailabilityArm`を作り、そのMediaSync instance専用の正の64-bit `armSequence`を1から単調増加で割り当てる。同じMediaSync instanceの寿命中は過去の`armSequence`を再利用しない。別MediaSync instanceではinstance identityとplayback generationが別の失効境界になるため、arm sequenceのglobal/session namespace、乱数nonce、live/retired token集合、collision checkは設けない。native MediaSyncはcurrent armの`armSequence`を保持するだけでavailability semanticsを解釈しない。arm中にvideo bufferがlate-drop分岐を通過し、current `mOutput`へのattachと`queueBuffer()`がともに成功した時点で、その**成功を判定したarmの`armSequence`をevent payloadへ固定してから**armを解除する。late-drop、attach失敗、queue失敗、output abandonment、inputへ返したbufferではeventを生成せず、arm状態も消費しない。re-arm後に旧eventのJava配送が遅延しても、event payloadのsequenceは生成時の旧armから書き換えない。64-bit sequence exhaustion／wrapを実運用上の回復経路として設計せず、これを理由に暗号乱数、永続retired集合、MediaSync再生成などの追加runtime recoveryを設けない。常時すべてのrender成功を通知せず、TISが必要な`AvailabilityArm`だけをarm/re-armする。

TISは**各accepted `onTune(Uri)`ごと**に新しいvideo availability obligationを生成する。videoを持つtuneではcurrent waiting armを取消してfresh `AvailabilityArm`を割り当て、各accepted `onTune(Uri)`ではcurrent waiting armを取消し、TIS playback graphを終了して新しいplayback generationと新MediaSync instanceのinitial `AvailabilityArm`を作る。TISはchannel同一性、frontend lock状態、pipeline healthを根拠に`Tuner.tune(settings)`の発行またはplayback generation再生成を省略しない。同一の正規化settingsで既存物理lockを安全に継続できる場合にbackend retuneを省略する判断は、frontend stateを所有するTuner HALだけが行う。当該tune受理後の新generationで生成されたcurrent final-output成功eventだけが、そのtuneの`notifyVideoAvailable()` obligationを満たす。audio-only serviceは既存契約どおり映像availabilityを偽装せず`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)`で閉じる。callback eventはMediaSync instance、playback generation、event payloadの`armSequence`を同executor上で照合し、sequenceがcurrent `waitingAvailabilityArm.armSequence`と一致し、current `sessionSurface`が有効、視聴制限でblockされておらず、同generationの`MEDIASYNC_ERROR_SURFACE_FAIL`がない場合だけ`notifyVideoAvailable()`を呼ぶ。受理またはarm取消し後はcurrent waiting armを無効化する。一度availableになった後、`VIDEO_UNAVAILABLE_REASON_BUFFERING`等のrecoverable unavailableへ遷移し**同じMediaSync instance/generationを維持して復旧する場合**もfresh armでre-armし、re-arm前の旧armで既に生成済みだった遅延eventはarm sequence不一致で必ず破棄する。fresh arm後のfinal-output成功eventだけで再びavailableへ遷移する。generation teardownを伴うunavailableは新MediaSync instanceのinitial armで閉じる。これによりper-`onTune()` obligationは常に新playback generationで閉じる。`available -> recoverable unavailable -> available`で同じMediaSync instance/generationを維持するre-armは、同一accepted tune内の一時的復旧だけに限定する。

Exact modeのcallbackは物理display/compositorへのpresent fence完了を意味せず、video scheduling/drop ownerであるMediaSyncがrender対象を選択しcurrent final outputへのqueueを成功させたことだけをcommitする。Exact modeでは`MediaCodec.OnFrameRenderedListener`、`MediaSync.getTimestamp()`、playback clock進行をavailability commitへ使用しない。native内部mutexを保持したままJavaへreentrant callせず、成功時に固定した`armSequence`を保持したままJNI/Java handlerへ非同期配送する。release済み、旧generation、旧MediaSync instance、またはTISのcurrent waiting armとarm sequenceが一致しない遅延eventはstate更新に使わない。Compatibility modeの`MediaCodec.OnFrameRenderedListener`利用は次段落で定義する限定fallbackであり、このExact mode契約を弱めない。

このcallbackはplatform-private contractである。ただしTIS APKはstock LineageOSでもbuild/run可能に保つため、このprivate listener型とsetterを静的参照せずruntime reflectionで解決して呼び出す。private listener経路が呼び出し可能な場合は`MEDIA_SYNC_FINAL_OUTPUT_EXACT`として上記final-output成功eventだけをavailability commitに使う。API不存在、reflection解決失敗、登録setter呼出し失敗その他の理由でprivate listener経路を呼び出せない場合は、公開APIの`MediaCodec.OnFrameRenderedListener`を型付きで使用し、MediaCodecから`MediaSync.createInputSurface()`へのframe renderを`MEDIA_CODEC_TO_MEDIASYNC_INPUT_COMPAT`として互換fallbackにする。このfallbackはMediaSync内部のlate-dropやfinal output `queueBuffer()`成功を証明しないためExactと同値に扱わず、診断で必ず区別する。platform patch適用、target build、実機確認などの製品統合条件は`INTEGRATION.md`を正とし、本書では再定義しない。

## EIT と TvProvider

現行releaseで収集するEIT table範囲、短期補完の用途、長期・他service・予約/追従利用のrelease境界は、tv直下の`開発規則.md`のr51到達点を唯一の正本とする。本書はそのscopeを再定義せず、TIS runtimeにおけるfilter起動・停止、Programs書き込み契機、retry、現在番組解決、視聴セッション利用だけを定義する。Programs の `internal_provider_data` には JSON v1 の stable `programKey`、start+durationのtiming、放送由来CAS意味事実、長形式イベント項目、component/audioメタデータ、series完全構造、`eventGroups`、linkage、free_CA_mode、ARIBレーティングraw値、診断JSONをTIS内部データとして保存する。音声言語は`components.audio[].language/secondLanguage`にだけ保持し、top-levelへ複製しない。runtimeで選択したaudio/video track、`TvTrackInfo.trackId`、Android canonical genre投影結果、Android rating文字列、decoder/CAS product capability、channel/EPG/live可否はprovider-dataへ保存しない。TvProvider の標準columnには title / short description / long description、broadcast genre、明示写像できる canonical genre、series id、episode display number、scrambled、audio language、コンテンツレーティングなど、`ARIB_SI_EPG_TvProvider投影方針.md`で自然対応が固定された範囲だけ反映する。last episode number は通常の `TvContract.Programs` 標準列へ投影しない。

TvProvider標準列への投影判断は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とする。`internal_provider_data` の schema、canonical encode、保存上限、parser/descriptor診断schemaは `arib_si_engine_rs/DESIGN_JA.md` と Rust serde 構造体を正とする。本書はTIS runtimeにおける取得、policy算出、書き込み契機、retry、現在番組解決、視聴セッションでの利用だけを定義する。

### 複数table instance収集と停止

複数のtable instanceを包括的・継続的に取得する必要がある操作では、TISは`TableInfo repeat=true`を使用する。Tuner HALに未知の全instance集合の列挙や終端推測を要求しない。

TISは`SiCollectionRequirements`で操作目的に応じた必要集合を作り、同一bulkのscope別table完成状態とEIT instance状態で判定する。global discoveryStageを操作完了の代理にしない。

| 操作 | 対象集合と必要instance | 更新・終了条件 |
|---|---|---|
| setup / explicit rescan | 同じcandidateのSDT actualに属する現在観測サービス。profileの必須SI集合のうち対象transportのSDT/NIT、対象サービスのPMT、およびPAT・profile必須補完表。EITは初期channel登録の必須にしない | 収集中のサービス追加・消失で集合を更新し安定待ちをやり直す。最短2秒かつ集合・完成状態が1.2秒安定し全必要instanceが完成すれば終了。登録可能な部分集合の安定による終了はSTABLE_PARTIALとする |
| boot EPG sync / background maintenance | 開始時に問い合わせた既存channelのServiceKeyをfrequency/deliverySystem/selector/satelliteBandの物理候補ごとに固定する。上記の対象SIと各ServiceKeyのp/f actual EITを必要にする。表示番号の違いで対象を落とさない | 対象の消失は未完成に残し、対象外サービスの到着で代用しない。EITを待たず安定部分終了せず、全必要instance完成または最大12秒で終了する。必要集合の完成とprovider transaction成功は別条件とする |
| live | 現在の選局世代のServiceKeyについてPSI/SI・EPG・CAの継続変化を監視する | 初回snapshotの完成を視聴中の更新監視の終了条件にしない。repeat=trueで監視を続け、選局変更・資源喪失・解放時に既存の終了処理でstopする。解析器の内部保持は有限collection寿命に従う |

live refreshは`LivePlaybackSnapshot`一つを取得し、その同じnative transactionから構成した`programs: ProgramPublishSnapshot`を含めてSessionが保持する。Program公開・rating・component/track・default component group・再生選択はその保持値だけを使い、処理途中に別bulkを再読しない。PMT filterは`pmtPids[currentServiceKey]`一件へ限定する。PAT/CAT等の共通SIとcurrent serviceのECM/EMMは既存scopeを維持する。CAS専用PIDのsectionはSI engineのadmission・SI更新通知を通さず、CasControllerへ直接渡す。不正・反復したSIは従来どおり有限予算へ算入する。

登録とliveで同じ`ServicePolicyEvaluator`および`TunerSelectionPolicy`の静的audio/video predicateを使う。`registrationReady`は許可service_type、current deliveryのSMD、PMT/PCR、codec signalingのresolved状態と対応種別、既存の識別・選局条件から決定し、CA descriptor解決状態は登録条件にしない。`caDescriptorsResolved`を独立した必須factとして判断結果へ渡し、`casDecisionReady = registrationReady && caDescriptorsResolved`、`clearLivePlaybackStaticallyEligible = casDecisionReady && !requiresCas`とする。CA未解決でも登録条件が揃えばchannel登録・EPG公開の候補にできるが、空metadataからclearを推測せずCAS処理・再生開始を抑止して旧再生を停止する。登録不適格時はEPG公開も抑止する。dynamic ECM/EMM PID集合はCAS判定を通った場合だけ採用し、不成立時は空集合へ置換して旧filterと配送を止める。PMTは判定更新に必要なので維持する。filter解放では配送sourceを先に失効させる一方、未解放artifactは既存handleのclosing状態で保持し、dynamic PID台帳はclose成功後だけ除去する。複数PIDやPMT/ECM/EMM群の途中失敗でも他の解放を試行する。失敗したPIDが再び必要になっても、旧handleの解放が成功するまで新handleを作らない。metadataとfilterの更新順序および失敗清掃は本書「CAS / descrambler の現行境界」の単一transactionに従う。CAS判断不成立時も空metadataと空ECM/EMM集合へ更新して再生を止め、caption終了・利用不能通知を省略しない。最初の失敗に後続失敗を添えて伝播する。free_ca_mode単独の診断は、構造が確定したPMTのCA有無を上書きしない。video付きserviceの未対応audioはvideo-onlyを許す。AVC descriptor不在は許容し、実SPSとの一致および実MediaCodec能力はlive configureで検証する。

現在番組がないことのauthorityはServiceKey全体のProgram key集合の空判定から導出しない。ARIB STD-B10 Annex 1.4.1に従い、current p/f actualのsection 0が現在版で受信済み・構造安全・無矛盾であり、同sectionのeventが存在しない場合にrating resolverへ型付き`AUTHORITATIVE_EMPTY`を渡す。section 1にfollowing eventがある場合や、following sectionが未受信の場合でも、確定した空presentをProviderの旧current行で上書きしない。section 0の未受信・構造不正・同版矛盾は`UNCONFIRMED`として既存Provider fallbackを維持する。eventの有無には通常候補と公開除外event factsの双方を使い、不正eventを空presentへ読み替えない。authorityが確定した空presentならUNRATED/no-current identityへ進む。同sectionにeventを観測した場合は型付き`PresentObserved`へそのeventを渡し、時刻区間の有無に依存せずProviderより優先する。単一の`DEFINED`/`UNDEFINED_TIME` presentの受信済みrating descriptorは既存mapperで写像し、未対応・欠落ratingはUNRATEDとして扱う。`UNDEFINED_TIME`ではServiceKey/eventIdを保持するが、start/endは両方nullとし、時刻を推測した一時解除を許さない。複数presentまたは診断専用の不正eventだけの場合はUNRATED/no-current identityとし、古いProviderへ戻さない。全体の削除用key集合と削除区間の既存policyは独立して維持する。

有限走査では成功・timeout・cancel・例外のいずれでも、最終snapshotを使う前に全section filterをstop/closeして読取りcallbackを無効化する。stop/close失敗は伝播し、収集完了成功に読み替えない。timeoutの部分成果から開始時のrequired ServiceKey全件を完了扱いしない。受信中に版・必要集合・完成状態が変われば同じsnapshotから再評価する。

## 字幕・文字スーパー表示の責務

ARIB 字幕と文字スーパーは TIS 側のcaption presentation pathで `libaribcaption` を使用する。PMTのdata componentから放送由来のservice kindを確定し、字幕はユーザー選択可能な`TvTrackInfo.TYPE_SUBTITLE`として通知して`onSetCaptionEnabled()` / `onSelectTrack()`へ接続する。文字スーパーは字幕とは独立したpresentation serviceとして保持し、同一ES/track stateへ畳み込まない。字幕と文字スーパーが同時に存在する場合は互いを排他にせず、独立overlay layerで同時表示可能にする。trackまたはpresentation serviceをadvertiseする場合は、対象ARIB PESをlibaribcaption C API経路で実際にdecode/renderできることを対応宣言条件に含める。

PMT `data_component_id=0x0008`は字幕と文字スーパーの共通値であり、それ自体をservice kind判定に使わない。`arib_si_engine_rs`がData Component Descriptorの`additional_arib_caption_info`を`DMF:4bit / reserved:2bit / Timing:2bit`として構造化した放送factを正とする。現行日本向けprofileでは`Timing=01`をprogram-synchronous caption、`Timing=00`をasynchronous superimpose、`Timing=10`をtime-synchronous superimposeとして扱い、`Timing=11`はreservedとして成功分類せず診断する。DMFはcaption/superimpose分類値ではなく表示modeであり、raw 4bitと受信時automatic-presentation等の導出factをservice kindとは別に保持する。固定byte値や`component_tag`範囲からTiming/DMFを推測しない。

caption management dataの`num_languages / language_tag / ISO_639_language_code / DMF / DC / Format / TCS / rollup_mode`等、TIF track discoveryとpresentation policyに必要な有限の管理fieldはTIS caption pathのRust JNI boundaryで構造化する。KotlinにSTD-B24のbinary parserを第二正本として持たず、字幕本文・DRCS・文字組版・renderingはlibaribcaptionへ委ねる。caption management dataのlanguage setを字幕/文字スーパー言語のSSOTとし、PMTのgeneric ISO639 descriptorを代用しない。STD-B24のgeneric syntaxとして`language_tag=0..7`は放送factとして保持できるが、現行ISDB-T/BS/CS110運用profileでplayableとしてadvertiseするのはTR-B14/TR-B15の同時最大2言語かつ現行JNIが選択可能な`language_tag=0/1`だけとする。parseできてもdecode/select不能な`language_tag=2..7`を`TvTrackInfo`としてadvertiseしない。management data更新でlanguage setが変化した場合は安定したtrack identityを維持できる範囲で`notifyTracksChanged()`を更新する。

字幕PESはprogram-synchronous経路として`TYPE_TS / SUBTYPE_PES`、字幕PID、明示`streamId=0xBD`（`private_stream_1`）で取得し、独立PES dataの`data_identifier=0x80`を検証する。文字スーパーPESはasynchronous経路として同じAOSP PES filter APIを使うが、文字スーパーPID、明示`streamId=0xBF`（`private_stream_2`）、`data_identifier=0x81`を別契約として検証する。captionとsuperimposeを同じ`0xBD` parser/filter pathへ流用しない。これらはTISが選ぶARIB利用設定であり、Tuner HALのPES capabilityを`0xBD/0xBF`だけへ制限する契約ではない。HAL正本は広告済みPES能力について有効な明示`streamId 0..255`、wildcard `0xFFFF`等のAOSP公開契約をそのまま満たす。

`arib_si_engine_rs` の自前ARIB文字列decoderはサービス名・番組名・番組説明など字幕以外のSI/EPG文字列に限定し、字幕/文字スーパーPES本文を渡さない。libaribcaptionはC APIのみを使用し、独自C/C++薄層は書かない。Kotlinから直接C APIを呼ばず、TIS Kotlin → Rust JNI boundary → 安全なRustラッパー → libaribcaption C APIの順に接続する。BML / data broadcast実行環境、双方向データ放送UI、データ放送UIは恒久対象外である。


## libaribcaption renderer runtime 契約

ARIB字幕・文字スーパー表示は、repoで供給される `libaribcaption-android` の製品forkをSoong build graphに入れ、renderer有効の `libaribcaption` moduleを正式経路として使用する。build/link/partitionの統合条件は `INTEGRATION.md` を正とし、本書はTIS内部runtime、renderer viewport、PTS、native lifecycleだけを所有する。out-of-graph prebuilt、renderer無効build、`dlopen()`確認だけ、decoder API呼び出しだけ、Canvas文字描画だけを字幕対応宣言条件にしてはならない。

`libmaleicacid_arib_caption_jni` は Rust JNI boundary + 安全なRustラッパーから libaribcaption C APIを直接利用し、TISは字幕/文字スーパーPESをservice kind、language、timing factとともにdecoderからrendererへ渡してRGBA8888 renderer出力を対応overlay layerへ接続する。字幕PESを受け取ってもrenderer表示に到達できない状態を字幕対応成功として扱ってはならない。renderer結果は、PTS、duration、image metadata、RGBAを含む長さ検査済みのone-shot packed result 1個としてRust-owned bufferからKotlin/Bitmap所有へ渡す。frame handle registry、imageごとのJNI往復、永続serializationは設けない。Kotlinへlibaribcaptionのraw pointerや借用寿命を漏らさず、caption/result/imageのcleanupはRust FFI境界で完結させる。

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

### program-synchronous字幕PTS scheduling / NoPTS / clear ownership

`Timing=01`のprogram-synchronous字幕について、字幕のmedia clockはvideo/audioと同じcurrent MediaSyncのcanonical clockだけとする。TIS、Rust JNI、`CaptionOverlayView`はPCR/wallclockから別media clockを作らず、固定delayや周期的な`getTimestamp()` polling loopも持たない。字幕PESにrendererへ渡せるauthoritativeな33-bit 90 kHz PTSがある場合だけ、video/audioと同じcurrent playback generationのunwrap規則でcanonical `timeUs`へ変換し、その時刻をdecoder/renderer/schedulerの同一caption時刻として使用する。PCR、wallclock、受信時刻、固定delay、直前caption PTS、nominal frame rateから字幕PTSを生成しない。

libaribcaptionの `ARIBCC_PTS_NOPTS` / `PTS_NOPTS` をrendererへappendしない。rendererが使用できるauthoritative PTSがないcaptionは、0へ丸めず、直前PTSをcarry-forwardせず、PCR / wallclock / MediaSync current position / 受信時刻 / nominal frame rateからcaption PTSを生成せず、renderer queueへappendせず、そのcaptionを表示成功として扱わず型付き診断へ記録する。NoPTS入力だけを理由に既に表示中の有効captionを即時clearせず、その既存caption自身のduration / clear / lifecycle契約に従う。現行製品profileが字幕表示対応を表明するには、字幕filter / producerが表示対象caption PESについてrendererに渡せるauthoritative PTSを供給できることをqualification条件に含め、満たせないbackend/profileはdecoderが文字列抽出できてもr51字幕表示成功対応として表明しない。

字幕display/clearは、MediaSyncの`getTimestamp()`が返すmedia time / anchor time / playback rateを唯一の時間基準とするevent-driven one-shot subtitle schedulerが担当する。新caption、finite-duration clear、明示clearのうち次の1境界だけをarmし、予定時刻到達時にcurrent MediaSync timestampを再読して境界到達を確認する。earlyなら同じ境界へre-armし、dueならdisplay/clearして次境界だけをarmする。周期polling、独立free-running clock、PCR→wallclock clock、video frame release/drop判定を実装しない。

libaribcaptionが有限`wait_duration`を返すcaptionは `PTS + duration` を同じcanonical timeline上の明示clear境界とする。`DURATION_INDEFINITE`は有限値へ推測変換せず、次caption、ARIB/libaribcaptionの明示clear、字幕track無効化、generation終了までcurrent imageを保持する。次captionはそのPTS境界で旧imageを直接replaceする。既表示captionのdurationを後から別clockで補正しない。

このschedulerはA/V clockやvideo schedulerを複製するものではなく、MediaSyncが所有するcanonical playback positionに字幕presentation eventを従属させるUI dispatch層である。

### 文字スーパー Timing / DMF / clock 契約

`Timing=00`の文字スーパーはasynchronous presentationとして扱い、PESにPTSが無いことを異常としない。受信・data-group構文・management/statement decodeがcurrent presentation epochで成功し、DMFが受信時automatic-presentationを指定する場合は、0、直前PTS、PCR、MediaSync position等の擬似PTSを生成せずcurrent overlayへ即時presentationする。non-automatic / selectable等のDMFはraw値とpolicy factを保持し、broadcaster指定のautomatic modeとユーザーの字幕enabled stateを同一booleanへ潰さない。文字スーパー用の表示可否policyは字幕track選択とは独立に扱う。

`Timing=10`の文字スーパーはSTD-B24のtime-synchronous modeとして、caption statement dataのTMD/STMをauthoritative presentation timeとする。STMをMediaSync PTSへ変換・捏造せず、TDTまたはTOTで校正された既存broadcast wall-clock authorityへ接続する。STD-B10の運用どおりPID 0x0014のTDT(table_id=0x70)とTOT(table_id=0x73)を同一のJST date/time authority入力として扱い、TOTではsection CRC_32を検証してから時刻factを公開する。STMは時刻fieldであるため、現在のbroadcast dateとの結合、日跨ぎ、TDT/TOT更新または不連続をbroadcast-clock generationで一意に扱い、OS受信wallclockを無条件な代替正本にしない。

同一generationの判定は、直前のbroadcast date/timeを受信monotonic経過時間だけ進めた予測値と、新TDT/TOTのbroadcast date/timeとの差で行う。TDT/TOTの時刻fieldは1秒単位なので、製品側continuity判定は2,000ms以内を同一generationとし、それを超える差またはmonotonic受信時刻の逆行をdiscontinuityとして新generationへ進める。この2,000msはARIBの放送時刻値を置換するclock精度ではなく、1秒量子化された連続sampleを同一authority generationへ束ねるreceiver policyである。通常更新と日跨ぎは同一generationのままpending STMを最新sampleでcancel/re-armし、discontinuityでgenerationが変わった場合は旧generationのpending STMを新clockへ再解釈せずfail-closedで破棄する。handler wake時にも同じgenerationを再確認してから表示する。clock authorityが未確定、STM構文不正、reserved TMD/Timingの場合はtime-synchronous presentation成功を表明せず型付き診断へ落とす。独立したfree-running clock、周期polling、caption用PCR clockを追加しない。

文字スーパーmanagement dataも字幕と同じcaption management構文factを取得し、最大2言語の現行日本profile capabilityへ接続する。superimposeであることを理由にmanagement parseを省略しない。DMF / Timing / language set / TMD / STMは同一PES presentation epochのbroadcast factとして扱い、libaribcaption decoder/renderer stateと別の永続SSOTを作らない。

### decoder / renderer / scheduler lifecycle

字幕と文字スーパーは別のpresentation layerとしてそれぞれnative state、scheduler state、overlay stateを一つのpresentation epochに閉じる。両layer間でselected track、DMF、language、current frame、pending eventを共有せず、同時表示を許す。各layer内ではdecoder queue用generationとUI runnable用epochを別々に所有せず、全失効イベントで単一epochを進める。

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

最低試験には、valid viewport確定前のrenderを成功扱いしないこと、valid viewportでRGBA8888 + dst rectがoverlayへ出ること、stride/buffer size検査、viewport変更で旧bitmap座標を流用しないこと、`Timing=01` captionが`0xBD / data_identifier=0x80`とauthoritative PTSでcanonical timelineに表示されること、caption NoPTSを0/前値/PCR/wallclock等で補完しないこと、NoPTS captionをrenderer append/display successにしないこと、`Timing=00/10/11`をcaptionへ誤分類しないこと、`Timing=00` superimposeを`0xBF / data_identifier=0x81`で受理して擬似PTSなしにDMFどおり表示すること、`Timing=10`でvalid STMとbroadcast-clock authorityを使い日跨ぎ/clock generationを処理すること、`Timing=11`をfail-closedにすること、caption/superimpose management dataからlanguage_tag 0/1だけをplayable trackとしてadvertiseし2..7を誤広告しないこと、字幕と文字スーパーを同時表示できること、NoPTS入力だけで既存有効captionを根拠なくclearしないこと、disable/deselect/track change/filter continuity loss/retune/playback generation/Surface generation/releaseの各境界でlayerごとのstate ownershipが成立すること、A/V-only plain flushではcaption/superimpose generationをresetしないこと、release後のstale resultが描画しないことを含める。

## ライブ playback 実装方式

TIS のライブplaybackは、Tuner AV filterのclear-memory `MediaEvent.getLinearBlock()`が示す各有効rangeをAOSP標準どおりMediaCodec block modelへ直接queueする。TISはcodec access-unit境界を再解析せず、fragmented ESを別`LinearBlock`へ再構成せず、通常ES payloadをByteArrayへcopyしない。video decoder出力を`MediaSync.createInputSurface()`へ、audio decoder出力PCMを`MediaSync.queueAudio()`へ渡す。MediaSyncはsession SurfaceとAudioTrackを所有し、A/V同期、映像提示時刻、音声clock追従を担当する。TISはMediaSyncの外側に独自clockまたは独自frame schedulerを置かない。

本productはnon-tunneled MediaCodec + MediaSync経路をarchitectureとして採用し、tunneled / platform passthrough playback capabilityを恒久的に提供しない。`notifyVideoAvailable()`は、Exact modeでは本書「MediaSync Framework-private final-output observation」で定義したcurrent availability epochのfinal-output成功eventだけをcommitにする。Compatibility modeでは公開`MediaCodec.OnFrameRenderedListener`のframe render時刻がcurrent arm時刻以後で、current codec/generation/Surface/error gateを満たす最初のeventを近似commitに使用する。Compatibility modeのeventはMediaSync input Surface到達の観測でありfinal-output成功とはみなさない。`MediaSync.getTimestamp()`のclock進行はどちらのmodeでもavailability根拠にしない。

setup scan の channel registration は global discovery complete を必須条件にしない。ただし partial snapshot を無条件に channel insert に使ってはならない。TvProvider のサービス単位の登録可否は本書の「サービス登録・公開・再生policy境界」を唯一の正本とし、この節で video ES 必須などの追加 gate を重複定義しない。したがって `service_type=0x01` は同節の audio-video / video-only 条件、`service_type=0x02` は対応 audio ES を持つ audio-only 条件に従い、`0x02` の登録に video ES を要求しない。登録可能未満の partial snapshot は診断情報 / ライブ更新 / debugにのみ使い、channel insertしない。scrambled サービスはchannel登録してよいが、current CAS capabilityとproduction tokenが成立しない状態を平文ライブ視聴成功として宣言してはならない。

## codec header / A-V sync / publish mode の固定

r51 のライブ playback codec は video=MPEG-2 video / H.264 AVC、audio=AAC / MPEG audio とする。r52では`開発規則.md`の到達点どおり、現行対象の従来TS profileでARIB signaling上HEVC/H.265が現れる場合を同じgeneric Tuner→MediaCodec経路の再生判定対象へ含め、PMT / descriptor認識、MediaFormat、MediaCodec capability照合、decoder起動、MediaSync first-output gate、unsupported診断を同一契約で扱う。codec追加を別の専用再生経路にせず、対象releaseのARIB本文選定規則とdecoder capabilityに従って同じdirect pathへ接続する。

STD-B79のISDB-T2 / ISDB-T1.5およびSTD-B80のISDB-T3は`開発規則.md`で恒久的な製品scope外とされているため、それらの方式だけに依存するcodecを本productのplayback capabilityへ追加しない。

本productがchannel登録およびlive viewableとして対応するARIB `service_type`集合は、`0x01`の`Digital television service`と`0x02`の`Digital audio service`だけに恒久固定する。`TvContract.Channels.COLUMN_SERVICE_TYPE`は`../ARIB_SI_EPG_TvProvider投影方針.md`に従ってARIB `service_type`のcodingを保持し、Android generic `SERVICE_TYPE_AUDIO_VIDEO` / `SERVICE_TYPE_AUDIO`へ意味変換しない。その他のservice typeはparser / provider-data診断ではraw値を保持するが、既知typeへ丸めず`UNSUPPORTED_SERVICE_TYPE`を記録してchannel登録・live viewable対象にしない。

`service_type=0x02`は本来的なaudio-only serviceである。少なくとも1本の現行対応audio ESと物理選局情報、`ServiceKey`、inputId、表示名が揃えば、video ESを要求せずARIB `service_type=0x02`のchannelとして登録し、audio filter・decoder・AudioTrackだけを開始する。視聴sessionでは映像filterを開かず、サービス分類確定後に`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)`を通知し、audio再生の成否と映像なし通知を分離する。audio codec非対応またはaudio ES欠落は`AUDIO_ONLY`の正常理由ではなく、`UNSUPPORTED_AUDIO_CODEC`または`SERVICE_TYPE_PMT_MISMATCH`として再生不能にする。

`service_type=0x01`はaudio-video serviceであり、対象releaseで対応するvideo ESがない場合にaudio-onlyへ再分類しない。弱信号またはlock喪失は`VIDEO_UNAVAILABLE_REASON_WEAK_SIGNAL`、有効なserviceでdecoder起動またはqueue補充を一時待機する場合だけ`VIDEO_UNAVAILABLE_REASON_BUFFERING`、video codec非対応またはPMT構成不整合は`VIDEO_UNAVAILABLE_REASON_UNKNOWN`と型付き診断`UNSUPPORTED_VIDEO_CODEC` / `SERVICE_TYPE_PMT_MISMATCH`へ分離する。HEVCはr51ではmetadata / 診断対象に留め、r52で`開発規則.md`の条件を満たす従来TS profileについてgeneric video playback selectionへ含める。

現行対応 video ES が存在し、audio ES が存在しない、または audio codec だけが現行未対応の場合は、video-only サービスとして視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。STD-B32 4.0以降の改定概要で高度地上デジタルテレビジョン放送向けに追加された MPEG-H 3D Audio / AC-4 は、STD-B79 / STD-B80 の高度地上方式が現行product scope外であるため現行codec固定表へ追加しない。AC-3 / Enhanced AC-3 も現行対象transportに対する条項根拠を確認せず推測で追加しない。

PMTからcodec family、audio/video種別、PIDを確定した後、AV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。snapshotは当該playback generationでTISが保持してよい`MediaEvent`の有限上限として、`singleEventLimitBytes`、`startupQueueBudgetBytes`、`startupQueueMaxSamples`、`startupQueueMaxDurationUs`、`pendingQueueBudgetBytes`、`pendingQueueMaxSamples`、`pendingQueueMaxDurationUs`、`decoderStartupDeadlineMs`、`steadyBackpressureDeadlineMs`を持つ。codec headerをまだ受信していないこと、またはdecoderが未構成であることを理由に開始済みgenerationの値を動的変更しない。値はcodec familyごとのTIS製品メモリ・待機時間policyであり、decoderが保持できる容量またはCDD/VTSの能力値を表明しない。したがってdecoder/device名別のprofile表やoffline測定結果をruntime入力として要求しない。実decoder/deviceとの適合は、`MediaCodecList.findDecoderForFormat`、configure、block-model queue、startup/steady deadlineの実結果で判定し、満たさない組合せはfail-closedにする。offline実機qualificationはrelease完了証拠として別に実施するが、その結果を固定queue上限へ転記して成功を捏造しない。snapshotはTIS側の保持量制御であり、最大上限相当の物理メモリをAV filter開始前に事前確保する契約ではない。

有限な`TisPlaybackBudgetSnapshot`を確定した後にAV filterを開始し、上限内の`MediaEvent`からdecoder構成に必要なcodec configuration metadataだけを取得して`MediaFormat`を構成する。r51ではMPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio headerを対象とし、r52でHEVCを再生判定対象にする場合はHEVC VPS/SPS/PPSも同じheader収集契約へ追加する。header解析に必要な最小rangeだけ`LinearBlock.map()`でread-only参照してよいが、通常payloadのqueue境界を作るcodec parserやES/AU copyへ拡張しない。decoder構成成功後は同じsnapshotのsteady-state上限へ遷移し、startup queueの各`MediaEvent` / `LinearBlock`は元rangeのままblock model QueueRequestへ渡してqueue成功時にcodecへ所有権を移す。runtimeで観測したdecoder block capabilityは各eventの投入可否と製品profile検証の診断にだけ用い、開始済み世代のsnapshotを書き換えない。AOSP direct-input契約を満たさないdecoder/profileではfilterを停止し、保持中のHAL handleを解放して`DECODER_CAPACITY_MISMATCH`または`DIRECT_INPUT_UNSUPPORTED`を記録し、成功対応として表明しない。

MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= LinearBlock capacity`を満たす場合だけstartup queueまたはblock model QueueRequestへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。

HEVCの起動用構成抽出は、Android APIと資源寿命を持たない`HevcConfigParser`へ分離する。入力は既存の有限header probeだけとし、通常ESのAU再構成には使用しない。`CodecBitReader`はAVCとHEVCで共有する。AOSP内部の`HevcParameterSets`は公開Java APIではなく、現在のTISはMedia3に依存していないため、起動時の寸法取得だけのためにnative接続やMedia3一式を追加しない。この選択は本プロジェクトの依存・保守範囲の判断であり、AOSPが独自解析器を要求しているという意味ではない。

HEVCのVPS/SPS/PPSは、後続開始コードまで受信してNALの終端を確認してからCSDへ収録する。開始コードの途中、NALの途中、必要なparameter setの未到着は構成待ちとし、既存の起動予算・期限だけを適用する。区切りまで受信したNALのheader不正、空のparameter set、SPS寸法までの構文切断、不正escape、予約値、不正寸法は`IllegalArgumentException`で構成不正を返す。これは既存の`VIDEO_CODEC_ERROR`と再生停止・資源解放へ接続し、`CODEC_CONFIG_TIMEOUT`へ丸めない。入力の3/4 byte開始コードを受け入れ、CSDでは各parameter setの開始コードを`00 00 00 01`に統一してVPS・SPS・PPSの順に`csd-0`へ格納する。根拠は[MediaCodecのCodec-specific Data契約](https://developer.android.com/reference/android/media/MediaCodec#codec-specific-data)とする。

このHEVC解析が保証する範囲は、NAL境界、収録するNAL header、SPSの寸法取得に必要な項目とcrop計算である。profile/tier/levelは寸法へ到達するため読み飛ばし、bit depth・VUI・全parameter set構文の適合判定は行わない。解析成功だけでMain10/HDR等の対応やHEVC全体への適合を表明しない。実decoderの選択・configure・入力・初回出力の結果を用いる既存の対応判定を維持する。

TISは保持中の`MediaEvent`についてevent数、payload byte数、presentation timestamp spanをsnapshotの有限上限内に制限する。range検証後にsingle-eventまたはqueue上限を超えるeventは原因別に`SAMPLE_TOO_LARGE`または`PENDING_QUEUE_FULL`を記録してHAL handleを直ちに解放する。保持量はbounded queueから算出しても、同じqueueに従属するO(1) counterで管理してもよく、独立した資源台帳や別generationを設けることを要求しない。generation変更、stop、releaseでは保持中eventを解放して保持量を0へ戻す。TISはAU再構成用の追加bufferを持たず、HALの`avPerFilterLiveBytes`または`avRuntimeBudgetBytes`等のAV backing/resource ledgerを公開・複製しない。

first frame前はcodec-specificな`decoderStartupDeadlineMs`を用い、必要なsequence header、SPS/PPS、audio config、reorder用入力を収集している間の一時queue増加を通常backpressure失敗へ写像しない。各AV filter開始成功時を起点として、入力量・queue超過・callbackの到着から独立した単調時計のタイマーを設定する。`decoderStartupDeadlineMs`までに最初の非空decoder出力へ到達しなければ、無入力・少量入力・構成済み無出力のいずれも起動失敗とする。期限時に未構成なら`CODEC_CONFIG_TIMEOUT`、構成済みvideoなら`FIRST_FRAME_TIMEOUT`、audioなら`AUDIO_UNAVAILABLE`とし、診断`DECODER_STARTUP_TIMEOUT`に失敗段階を残す。video失敗は全filterと保持sampleを停止・解放する。audio-only失敗も停止して利用不能を通知し、audio-videoのaudio失敗は既存のvideo-only新generation再生成規則へ進む。最初の非空出力またはdecoder closeでタイマーを解除し、旧generationのタイマーは現generationへ作用させない。plain Filter.flush / RestartEventは起動期限を延長しない。first frame後は別の`steadyBackpressureDeadlineMs`を用い、単発超過は当該sampleを解放して継続し、期限中にdequeue進行がなくqueue上限が継続する場合だけunavailableへ遷移する。audioだけの超過はvideo-only継続可否を既存規則で判定し、無条件にvideo unavailableへ写像しない。

A/V同期方式はAndroid標準`MediaSync`に固定する。本productはnon-tunneled playbackを恒久architectureとして採用し、tunneled playback、platform passthrough、`avSyncHwId`をTIS capabilityとして提供しない。`PlaybackPipeline`のserial executorが、現generationのMediaSync、MediaSync input Surface、session Surface、AudioTrack、video／audio decoder、未返却audio buffer id、playback rateを単一所有する。decoder callback、MediaSync callback、AudioTrack／route callbackはstateを直接変更せず、同executorへ直列化する。

videoは`MediaSync.setSurface(sessionSurface)`の後に`MediaSync.createInputSurface()`を一度だけ呼び、そのSurfaceをvideo decoder出力先とする。decoded outputは元PTSをナノ秒へ変換してMediaSync input Surfaceへrenderする。TISは`AudioPlaybackClock`、`StandalonePlaybackClock`、`VideoFrameScheduler`、`AudioTimestamp.framePosition`由来の独自media position、独自future frame保持、独自late drop閾値、独自renderTimestamp算出を実装しない。

audioはsession固有Contextで作った`AudioTrack`を`MediaSync.setAudioTrack()`へ設定し、audio decoderの現generation output PCMをPTS付き`MediaSync.queueAudio()`へ渡す。block model audio outputの`OutputFrame.getLinearBlock()`は必要範囲をmapし、返されたByteBufferをMediaSyncへ渡す。MediaSyncから`onAudioBufferConsumed(sync, buffer, bufferId)`が返るまで、該当codec output index、OutputFrame、LinearBlock、ByteBuffer、budget claimを保持し、変更・再利用・releaseしない。callback後に対応するcodec outputを非描画releaseし、所有権とclaimを返す。

MediaSyncは生成時のplayback rate 0を用いて必要な有限prefillを行い、視聴制限gate、Surface有効性、decoder開始、最小startup条件成立後に`PlaybackParams`のspeed 1.0で開始する。video-onlyではAudioTrackを設定せずMediaSync video経路を使い、audio-onlyではSurfaceとvideo decoderを設定せずMediaSync audio経路を使う。MediaSync errorは`MEDIASYNC_ERROR_SURFACE_FAIL`と`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`を区別する。surface失敗はvideo経路を持つサービスだけでvideo unavailableへ写像する。audio失敗は、audio-videoサービスでvideo経路を継続可能な場合でも旧MediaSyncを再利用せず、旧AudioTrack/audio decoderを含む現playback generationを終了し、MediaSync・video decoder・video filterを新generationとしてvideo-only構成で再生成する。診断に`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`を残す。audio-onlyサービスでは代替video経路が存在しないためvideo-onlyへ遷移せず、audio decoder、AudioTrack、MediaSyncと未返却bufferを回収して現generationを再生不能状態へ遷移し、`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`は映像が存在しないというサービス属性の通知にだけ使用してAudioTrack失敗理由と混同しない。video-onlyサービスはAudioTrackを設定しないため`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`遷移を持たない。

再生signatureにはPID/stream_typeに加え、decoder構成を変えるcodec種別、AVC signaling、AAC ASC/header、audio component typeをcanonical identityとして含める。診断用raw descriptorやprofile表示textだけの変更では再起動しない。AudioTrack生成時にtyped `AudioRouting.OnRoutingChangedListener`を登録し、playback executorでcurrent generationとAudioTrack identityを照合する。初回のroute確定と停止中のnullは変更扱いにせず、実route IDの変更だけを既存full restartへ渡す。listenerはtrack release前に解除し、旧callbackは無効化する。route/PCM変更とvideo-only fallbackの新generation結果は同じtyped callbackでSessionの再生状態・字幕世代へ反映する。

accepted live `onTune()`、service・codec/PID graph変更、明示的playback generation変更、stop、Surface変更、AudioTrack切替／再生成、audio route変更、decoder再生成では、既存MediaSyncの内部anchorを再利用せず、playback rateを0へ戻して未返却audio bufferと旧decoder outputを回収し、MediaSync input Surface、MediaSync、decoder、AudioTrackを解放して新generationとして再生成する。TISはfrontend lock/healthやchannel同一性を観測してこのfull resetを省略しない。各accepted live `onTune()`でTuner SDKの`Tuner.tune(settings)`を呼び、同一settingsで物理frontendを継続利用できるかはTuner HALの既存frontend tune state machineに委ねる。plain `Filter.flush()`はAOSP契約どおり未消費filter dataのclearに限定し、未queue `MediaEvent` / `LinearBlock`と対応claimだけを破棄してMediaSync/decoder/AudioTrack/playback generationを再生成しない。PTS raw deltaの大小、通常33-bit wrap、presentation-order reorderだけでもMediaSync generationを再生成しない。旧generationのdecoder／MediaSync／route callbackはstate更新に使わず、旧bufferを非描画解放する。

TISが受け取る`MediaEvent.getPts()`は、producerが当該media outputへ対応付けて確定したauthoritative metadataとしてopaqueに扱う。HAL producerが対応codec headerを構造検証し、PESを跨ぐbounded residual、当該PES内で最初に開始するAU、actual sample rate、exact sample countからこのassociationを確定する経路は、TIS側で禁止するgeneric timestamp interpolationとは別のproducer責務である。TISはそのcodec parser／associationを再実行・複製せず、`isPtsPresent`を元PES headerのprovenanceとして保持したまま、`isPtsPresent=false`でも配送されたauthoritative `getPts()`を通常consumer pathへ透過する。本書はproducer側の個別実装が完成済みであることを主張せず、公開された値をTISがどう消費するかだけを固定する。

最低試験契約は、AOSP Tunerのclear non-passthrough media filterから得た`MediaEvent.getLinearBlock()`の有効rangeをcodec別AU解析なしでblock model `QueueRequest.setLinearBlock()`へ直接queueすること、ESまたはpartial ESのevent列をTIS側で再構成・copyしないこと、MPEG-2 Video / H.264 / AAC / MPEG audio（r52では対象条件を満たすHEVCを追加）の選択decoder/deviceでこのdirect-input契約を満たすことを確認する。`isPtsPresent=true`では同じeventの`getPts()`だけをevent-level timestampとして使い、offsetをPTS位置と解釈しないこと、`isPtsPresent=false`のeventもpayloadをdropせず、0 / 前値 / PCR / wallclock等からPTSを捏造せず、別eventまたはcodec AUへPTSを再関連付けしないことを確認する。`PtsEpochCoordinator`は`isPtsPresent`をseed / advance / join条件に使わず、producer-authoritative `getPts()`を持つ全non-empty eventで進める。track-local `rawPrev` / `extendedPrev`とgeneration共有wrap epoch、`2^33-1 -> 0` wrap、`0 -> 2^33-1` reorder、半周期差=`-2^32`、generation最初のeventが`isPtsPresent=false`でもauthoritative `getPts()`を持つ場合、後発video/audio trackが`isPtsPresent=false`から開始する場合、generation開始時video=`2^33-100` / audio=`50`と逆順、双方が33-bit wrap近傍で開始する場合、後から開始／置換するtrackのjoinを検証し、A/V timeline差が本来の差を維持することを確認する。PTS deltaの大小だけではgeneration resetしない。plain `Filter.flush()`は未queue event / claimだけをclearし、coordinator / seed済みnormalizer / decoder / MediaSync / AudioTrack / generationを維持する。decoder再生成 / AudioTrack切替・再生成 / audio route変更 / Surface変更 / codec・PID・track graph変更を伴うretuneはfull generation resetにする。`MediaEvent`のlong offset / lengthについて負値、0 length、`Int.MAX_VALUE`境界、`Int.MAX_VALUE+1`、checked-add overflow、end==capacity、end>capacityをnarrow前に検査し、通常payloadをByteArray・別LinearBlock・通常input-bufferへcopyしないことを確認する。MediaSync rate-0有限prefillからstartup gate成立後speed 1.0へ遷移すること、`MEDIASYNC_ERROR_SURFACE_FAIL`と`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`の分離、audio-videoでAudioTrack failure時に旧MediaSync generationを破棄してvideo-only新generationへ再生成、audio-onlyでの再生不能遷移、video-onlyがAudioTrack error遷移を持たないこと、各accepted video `onTune()`でfresh availability armを作り、そのarm後のfinal-output成功前はvideo availableにしないこと、late-drop / attach失敗 / queue失敗でcurrent armを消費しないこと、成功event後だけ一回availability通知すること、`available -> recoverable unavailable -> available`で同MediaSync instanceを維持する場合にfresh armでre-armして次のfinal-output成功後だけ再availableにすること、arm Aの成功event配送を遅延させたままunavailable遷移とarm Bのre-armを行い遅延Aをarm sequence不一致で破棄すること、generation teardown後は新instanceのinitial armを使うこと、audio bufferのconsume callbackまでの寿命、A/V同期、video-only、audio-only、destructive retune / Surface変更 / AudioTrack切替・再生成 / audio route変更 / decoder再生成後のMediaSync再生成、各accepted `onTune()`で新playback generationを作り、そのgenerationのfresh initial arm後のfinal-output成功でavailabilityを通知すること、同一MediaSync instance内で旧arm sequenceを再利用しないこと、route変更後の旧generation非利用、MediaSync error写像、stale generation非描画を含む。試験のqueue数値上限はcodec familyごとの`TisPlaybackBudgetSnapshot`と一致させる。

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

- LineageOS 22.1／Android 15の通常ライブセッション生成では`onCreateSession(inputId, sessionId, tvAppAttributionSource)`をoverrideする。framework由来`sessionId`は`Tuner(serviceContext, sessionId, useCase)`へ渡し、`tvAppAttributionSource`はsession固有Contextの生成へ渡す。2引数版`onCreateSession(inputId, sessionId)`と1引数版は明示的な互換経路だけに限定し、対象productの通常3引数入口を素のservice Contextへ委譲または後退させない。
- r51 の video 対応宣言対象は MPEG-2 video `0x02` と H.264/AVC `0x1b` とする。HEVC `0x24` はr51ではmetadata / 診断へ保持し、r52で`開発規則.md`の到達点どおり、現行対象の従来TS profileでARIB signaling上現れる場合をgeneric Tuner→MediaCodec playback selectionへ含める。r52のHEVC対応は規範対象の現行ARIB原文、検証証拠の版・条項と未証明差分、MediaFormat / decoder capability / first-output gate / unsupported診断を同じ契約で固定する。
- ARIB 視聴年齢制限は raw `parental_rating_descriptor.rating` の意味を保ったまま Android `TvContentRating` へ写像する。`country_code=JPN` の `0x01..0x0F` は `age=raw+3` で AOSP system-defined `com.android.tv / ISDB / ISDB_4..ISDB_18`、BS/CSで運用される `0x10..0x11` は同式で `ISDB_19..ISDB_20` とする。明示的に受信した `0x12..0xFF` は年齢へ推測変換せず、product rating provider が定義する `com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` へ写像する。`0x00` と未対応countryはAndroid ratingを捏造せずraw値と診断に残す。`TvContentRating.UNRATED` は TvProvider current Program と latest EIT の双方から現在コンテンツに適用可能なratingが得られない場合だけに使用し、明示的な `0x12..0xFF` の代替値にはしない。
- `notifyVideoAvailable()` は正規製品のExact modeではcurrent MediaSync availability epochでlate-dropを通過しcurrent final outputへのattach＋`queueBuffer()`成功後に発行されるFramework-private first-output eventを受け、current Surface有効、generation一致、視聴制限、Surface errorのgateを満たした場合だけ呼ぶ。private callbackをruntimeで発見できないCompatibility modeでは、公開`MediaCodec.OnFrameRenderedListener`を型付きで使い、current codec/generationかつcurrent arm時刻以後のframe renderを近似根拠として使用するが、これはMediaSync input Surface到達であってfinal-output成功と同値ではない。`MediaSync.getTimestamp()`のclock進行はavailability根拠にしない。
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
- ARIB exceptional ratingのpolicy ownerはLive TV Appとする。rating定義は独立`AribContentRatings` APKがTIF標準providerとして公開し、System TV App本体へ直接patchを当てない。Android 15 / LineageOS 22.1系の既存`ContentRatingLevelPolicy`はTIF rating-provider XMLの`contentAgeHint`をpreset policyの入力として使い、`HIGH`は6以上、`MEDIUM`は12以上、`LOW`は各rating system内の最大age hint以上をblocked候補へ投影し、`NONE`は空集合にする。`ARIB_EXCEPTIONAL`は単一rating `BROADCASTER_DEFINED`を`contentAgeHint=12`で公開するため、既存policyでは`HIGH/MEDIUM/LOW`の各presetでblocked候補に含まれる。ここでの12はproduct preset policy分類用metadataであり、ARIB raw `parental_rating_descriptor.rating`を12歳へ解釈・変換した値ではない。明示受信した`0x12..0xFF`は従来どおり全て同一canonical exceptional ratingへ写像する。`CUSTOM`はstock TV Appの通常blocked-rating編集を正とし、このextensionがprivate stateを読んで強制上書きしない。第二policy APK、TV App private state reader、System TV App source patchを追加せず、PIN認証済みcurrent contentの`onUnblockContent()`一時解除と他domain/ratingSystemのpolicyを変更しない。TISはraw値から独自policyを実装せず`TvInputManager.isRatingBlocked()`の結果だけに従う。
- Live session は、上記section 0の現在番組解決を先に適用する。present authority未確定の場合の時刻定義済みlatest EIT cacheとTvProvider current Programは、現在時刻の半開区間`start <= now < end`で絞り、`ServiceKey + eventId`とstart/endで同一番組・同一放送回を判定する。current generationのEITは、選局時にresetされgeneration違いのsectionがTunerControllerで破棄された後に受信した観測なので、同一放送回の保存済みProgramより新しいrating情報として優先する。eventIdが同じでもstart/endが変わった場合、またはeventIdが変わった場合も、旧Provider rowのratingを新しい放送回へ継承せずcurrent generationのEITを採用する。current generationに現在番組EITが無い場合だけTvProvider current Programへfallbackし、両方に現在時刻へ適用できる番組が無い場合だけ`TvContentRating.UNRATED`を使う。TvProvider query自体の失敗は情報不存在と同一視せず、直前のparental access状態を保持する。
- parental blocked の通知は `notifyContentBlocked(rating)` と AV停止を主とし、parental block の通知手段として `notifyVideoUnavailable()` を呼ばない。
- `onUnblockContent()` の解除範囲は同一 `channelUri + serviceKey + eventId + ratingString` の現在番組 / レーティングに限定する。start/end は stable identity ではなく、解除対象が現在表示中の同一 Program row であることを確認する補助条件としてのみ使ってよい。start/end/duration を provider-data `programKey`、unblock stable identity、または Program identity の SSOT にしてはならない。

一時解除はSession executor内の`TemporaryContentUnblocks`だけが所有する。現在番組のstable identity変更、現在番組消滅、retune、releaseで失効する。解除の受理時点の番組終了UTCと、受理時の単調時計から換算した終了期限を固定し、いずれかに到達した時点で失効する。終了不明・期限算出overflow・既終了は解除を受理しない。番組時刻更新や同じ解除通知の重複では期限を延長しない。新たに観測した終了が早い場合は期限を短縮する。壁時計の後退でも単調期限を維持する。失効タイマーはsession executorへ再評価をenqueueし、旧タイマーは参照一致で除外する。期限通知を予約できない場合も解除を保持しない。これにより同一event_idの再使用に旧解除を引き継がず、開始時刻をstable identityへ追加する必要はない。終了後の延長番組を解除する場合は新たなframeworkの認証済み通知を必要とする。
- CAS plugin/session/ECM/token/descramblerのいずれかが成立せず再生成功にしない場合は `TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` を使う。初回映像、filter、codec、audio等の非CAS failureをこのreasonへ丸めない。
- `requiresCas`はcurrent `ServiceSemanticFacts`のCA descriptor等から得る放送由来意味事実とし、`unsupportedCas` / `clearLivePlaybackSupported`はcurrent product/CAS capabilityからTISがその都度算出する。既存channel/Program `internal_provider_data`の旧policy値をcurrent policyの代替参照に使わない。

## TIS / EPG 公開境界

現行の EIT publish/delete 対象は、TvProvider に channel が存在する `ServiceKey`、または同一 setup/rescan transaction で channel insert が成功して channelId が確定した `ServiceKey` に限定する。ライブセッション の `currentService` だけには限定しない。Channelの存在・所有権・ServiceKeyの一致を公開の前提とする。新規Programの作成は既存Program行の存在を要求しない。更新・削除はその所有Channelの既存Program行だけを対象とする。

現行r51の EIT publish/delete 対象 table は present/following actual `0x4E` のみとする。present/following other `0x4F`、schedule actual `0x50..0x5F`、schedule other `0x60..0x6F` は r51 の Programs publish/delete 対象外であり、更新区間を発生させない。r53以降で対象を拡張する場合は `開発規則.md` のrelease scopeを先に更新する。

EIT 更新時の update/削除区間は、追加・変更・削除された event の既存 `[start,end)` と新 `[start,end)` の union とする。現行仕様では長期固定 lookahead window を導入しない。長期 EPG lookahead window を扱う場合は、EIT scope / version / event identity / authoritative 条件を設計正本へ固定してから併用する。EIT table scope の version 変更で既存 section が消えた場合は、消えた event の既存 window も廃止行削除対象に含める。

公開判断はKotlin `EpgPublicationPolicy`が所有し、`EpgSectionPolicy`の同じsection選択を収集完了判定と共有する。Rustのcurrent/next別instance事実からcurrent actual p/fを選び、地上波は0..last、BS/110CSは0..min(last,1)の受信・整合・safeSectionsを確認する。Program行の対象はDEFINED、削除のvalid identity集合にはDEFINEDとUNDEFINED_TIMEを採用する。両時刻未定義・構造破損・未完成・同版矛盾は削除権限を与えない。未知descriptorのUnsupportedValueだけでは削除を抑止せず、raw診断を保持する。`EventModelMapper`は同じpolicyの行採用判定を利用し、独立したscope/timing基準を持たない。

policyはcollection内で観測した完成版の旧・新時刻境界だけをServiceKey別に保持する。windowはそのunionを使い、キー集合・deletionAuthoritativeは毎回current instanceから作り直す。collectionGeneration/profile変更時に旧境界を破棄し、未完成版ではwindowを返さない。時刻未定義は旧区間を保護するキーとして保持し、正常空EITはcurrent完全状態として扱うが、区間がないときに削除区間を捏造しない。`ProgramPublishSnapshot.authoritativeProgramKeysByService`は同じ判定を通った現在のキー集合で、正常空EITの確認に使う。再試行の根拠は、旧要求区間全体を覆う現在のauthoritativeな`updateWindows`とする。

Direct Boot保留の正式状態を`DirectBootEpgPending`とする。`DirectBootGuard`がdevice-protected storage上のこの状態を唯一所有し、boot EPG sync要求を受理した時点または未完了・失敗終了時に設定する。`ChannelScanManager`はJobSchedulerのschedule/cancelだけを担当し、pending、inputId、Contextのshadow stateを持たない。JobServiceは開始時に自TISのinputIdを再解決する。状態はprocess restartとuser unlockをまたいで保持し、background maintenanceは設定・解除しない。

`BootEpgSyncCoordinator` は Tuner や SI collection を開始する前に、解決済みの自 TIS `inputId` を使って既存 `TvContract.Channels` を必須問い合わせとして取得し、今回の boot EPG sync の authoritative target channel 集合を確定する。この必須問い合わせ自体が失敗した場合は channel なしとは扱わず `DirectBootEpgPending` を維持して再試行対象にする。問い合わせが正常終了し、自 TIS 所有の既存 channel が 0 件だった場合は、boot EPG sync に更新対象が存在しない `NO_WORK` 正常終了とする。この場合は Tuner、SI collection、Programs publish/delete を開始せず `DirectBootEpgPending` を解除し、JobScheduler の再試行を要求しない。setup / explicit rescan はこの `NO_WORK` 判定とは独立した channel 登録経路であり、boot EPG sync は 0 件状態から channel を作成しない。

既存 target channel が 1 件以上ある場合は、開始前のauthoritative `targetSnapshot`から得た`ServiceKey`集合をそのtaskのcompletion ledgerの正本として固定する。同一frequency/selectorへ複数channelをdedupしたscan candidate数をtarget完了数の代理にしない。同一boot EPG sync taskがcancelされず、各required `ServiceKey`について`collectSiForCandidate()`の有効なcollection結果から対象Program transactionへ到達し、必要なTvProvider必須問い合わせとinsert/update/deleteが成功commitしたことをledgerへ記録し、開始時required集合の全`ServiceKey`がcommit済みになった後にだけ`DirectBootEpgPending`を解除する。1 candidate内でA/B serviceのうちAだけがcommitした場合、candidate自体が成功していてもBを完了扱いしない。provider query/write failure、publish fingerprint生成失敗、cancel、target channel が存在するのにrequired serviceの登録可能事実が得られず、またはpublish可能Programと後述の正常空EITのどちらも確認できない場合は保留を維持する。candidate成功数、1件以上のservice commit、部分write、fingerprint cache更新だけを解除根拠にしない。したがって `NO_WORK` は「開始前のauthoritative channel queryが正常終了し、その結果が0件」の場合だけであり、受信失敗やSI不完全、policy不足を0件成功へ丸めない。

正常な空EITは受信失敗と区別する。同一taskで、required ServiceKeyの現在の登録可能事実と、矛盾・不正のない完成したp/f actual instanceを確認し、そのinstanceにeventが一件もない場合は、そのServiceKeyを検証済み空対象として扱う。以前の有効更新区間があれば通常のauthoritative削除を実行する。更新区間がない初回の空EITでは区間を捏造せず、所有channelとそのchannelの既存Programsの必須問い合わせが両方成功した後、書込み不要の確認済み対象としてcompletion ledgerへ記録する。既存Program行は区間なしの空EITだけでは削除しない。問い合わせ失敗・不完全なEIT・不正event・時刻未定eventを空成功へ丸めない。開始時channel 0件の`NO_WORK`とは異なり、対象ごとの受信とprovider確認を必要とする。boot時は同じ正常空EITの再確認にも現在taskのprovider問い合わせを省略しない。

最低試験は、(1) authoritative channel query failure では Tuner を開始せず pending を維持して再試行すること、(2) query 成功かつ自TIS所有channel 0件では Tuner / SI collection / Programs publish-delete を開始せず `NO_WORK` として pending を解除し再試行しないこと、(3) target channel が1件以上あるが全candidate失敗、またはpublish可能Program・検証済み正常空EITの両方がない場合はpendingを維持すること、(4) target channel が1件以上ありpublish transactionが正常commitした場合だけ通常成功としてpendingを解除すること、を含める。

登録可能サービスは、`ServiceKey`、物理選局情報へ戻せるchannel provider-data、`Channels.COLUMN_INPUT_ID`として保存する自TISのinputId、表示名が揃い、TvProvider channel insert/update に進めるサービスとする。input ownershipのSSOTはprovider-dataではなく`Channels.COLUMN_INPUT_ID`とする。表示名は `ChannelRecord.displayName` が nonblank ならそれを使い、なければ SDT service_name、さらに無ければ `service-<onid>-<tsid>-<sid>` を使う。この代替表示名は登録可能判定上の有効な表示名と扱う。

## CAS orchestration failure境界

scrambled unsupportedサービスでも、PMT/CAT/CA情報と診断を使って EPG / Programs / レーティング / provider-data は更新する。ただしcurrent capabilityを持つMediaCas plugin、session、ECM成功、valid session ID token、Tuner descrambler接続がすべて成立しない限り再生成功にしない。CAS起因のunavailableだけを `VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` へmapし、初回映像到達timeout、filter start failure、非対応stream、codec失敗、audio失敗をCAS unknownにmapしない。

CAS可否はcurrent `ServiceSemanticFacts`とcurrent CAS implementation/capabilityからTISが算出する。provider-dataに保存するのはCA descriptor/free_CA_mode等の意味事実だけであり、旧`unsupportedCas` / `clearLivePlaybackSupported` / `publishStateSource`をcurrent判定へ再利用しない。

Descrambler API の `setKeyToken()`、`addPid()`、`removePid()` は戻り値が `Tuner.RESULT_SUCCESS` の場合だけ成功とする。非 SUCCESS result は CAS 診断 failure として扱い、成功扱いで握り潰してはならない。

各`MediaCas.Session` / `Descrambler` contextは、current PMTから得た`desiredElementaryPids`と、`addPid()` / `removePid()`成功で確認できた`linkedElementaryPids`を分離して同じ`CasSessionState`内に保持する。metadata refreshはdesiredだけを置換し、成功したPID操作だけをlinkedへ反映する。失敗した差分は同一metadataの次回refreshでも残して再試行する。初回key linkの一部PID追加に失敗してrollbackする場合も、成功した`removePid()`だけをlinkedから除き、`clearKeyToken()`成功後だけ`keyLinked=false`とする。contextを`READY`とできるのは`ecmReady && keyLinked && desiredElementaryPids == linkedElementaryPids`のときだけであり、要求PIDの一部だけが実際に接続された状態を再生成功へ写像しない。直近ECMの失敗、無効token、診断のみの結果では`ecmReady=false`とする。metadata再通知だけでは回復させず、同一contextの新たなECM成功時だけ回復する。鍵の所有を表す`keyLinked`は解放成功まで保持し、readinessとは分離する。ECM結果も既存section後の判定へ即時通知し、未準備時はAVを停止する。遅延first-outputでREADY未成立の世代を可用通知しない。この状態は既存`CasSessionState`と単一executorの内側だけで所有し、別queue、worker、retry timerを設けない。

最低確認観点は、B25でECM/EMM、B1でECM-onlyとなること、B1 CAT/EMMをfilter/process対象にしないこと、ECM成功後だけsession ID bytesを同一のままtokenへ使うこと、不正/VOID/診断tokenを拒否すること、retune/closeとECM callbackのraceでstale token/PIDを新generationへ適用しないこと、非SUCCESSのTuner結果でvideo availableを通知しないことを含む。PID差分については、add失敗後とremove失敗後に同一metadata refreshで再試行して成功後だけ`READY`になること、初回key/PID部分成功を`READY`にしないことを含む。

## TvProvider failure semantics

TvProvider query failure と channel なしは別状態として扱う。既存 channel query が失敗した場合は `skippedNoChannel` として扱わず、failure診断とし、publish fingerprint更新・`DirectBootEpgPending`解除の根拠に使わない。

TvProvider query は必須問い合わせと任意問い合わせを区別する。チャンネル・番組の追加または更新、廃止行削除、既存チャンネル・番組検索、Direct Boot準備完了判定に使う query は必須問い合わせとする。必須問い合わせで `ContentResolver.query()` が null cursor を返した場合は `TvProviderQueryFailure` とし、empty resultとみなさない。`TvProviderQueryFailure` が発生したサービス/windowでは channel insert、program insert/update、廃止行削除、publish fingerprint cache更新、`DirectBootEpgPending`解除に進まず、再試行区間を保持する。provider-dataはcurrent policyのfallback sourceにしないため、policy判定のためのprovider-data代替参照queryを設けない。

Programs publish/delete が provider failure になった場合は、`ProgramPublishCoordinator`のprocess-local queueに`ServiceKey + windowStartMs + windowEndMs`の再検証要求を保持する。entryはnotBeforeMsと診断用failure classだけを持ち、旧EpgUpdateWindow・旧validProgramKeys・旧deletionAuthoritativeを保存しない。固定cooldownは60秒。次回publish entrypointで期限到達した要求を取り出し、同じ入力snapshotのauthoritativeな更新区間が同一ServiceKeyの旧要求区間全体を含む場合だけ、その更新区間の現在のキー集合から削除権限を再構成する。ServiceKey単位のキー集合だけでは旧区間の範囲を証明できないため再試行しない。実行可能な再試行がある場合、過去のpublish fingerprintとの一致による早期終了を禁止し、現在のauthoritative windowに対するprovider処理の成功後に、その区間と一致する要求を除去する。同一区間の非authoritativeな通常upsert成功では、未実行の廃止行削除要求を除去しない。未完成・不整合・期限切れcollectionなどで現行の根拠がなければ削除せず要求を保持する。entrypointなしにwake-upしない。成功したkeyは削除、失敗したkeyは固定cooldownで末尾へ戻す。attempt段階、jitter、retention timer、failure class別queueは設けない。process restart時は破棄し、boot/background syncの再収集を正とする。失敗をpublish fingerprint更新や`DirectBootEpgPending`解除の根拠にしない。

再試行の回復時間には上限を保証しない。長時間同一番組を視聴しSIの内容が変化しなくても、受理したSI sectionの通知から `TunerController.onSectionIngestedCallback → MaleicacidLiveSession.refreshDynamicSiAndCasFilters → publishLiveProgramsForCurrentService` を経て再試行入口へ到達する。ただし登録可能な現在snapshotと旧要求区間を覆うauthoritative windowが必要であり、受信停止や根拠不足では実行しない。60秒はcooldownであって受信・JobScheduler・TvProviderの成功期限ではない。プロセスだけの再起動はboot broadcastと同一ではなく、旧process-local要求は復元しない。再作成されたlive sessionの受信、利用者のscan、既に登録されたboot/background job等による再収集を待つ。DirectBoot pendingがない状態ではBootEpgSyncSchedulerは新jobを登録しない。再起動だけで再収集が必ず始まるとは保証しない。

dirty-window queueは全体上限512 windowsの単一LRUとする。超過時は最古entryを破棄し、ServiceKey別`droppedRetryWindowCount`を加算する。ServiceKeyごとの第二上限は設けない。process restart後はcounterを0に戻す。

SDT-other / NIT-other / BAT 由来で現在 candidate の actual transport に解決できないサービスは、現在 candidate の物理情報で channel insertしない。未登録で Program row が存在しない unresolved transport は scan/maintenance 診断情報に `skippedUnresolvedTransportCount` として記録し、Program provider-dataには書かない。unresolved transport はTISのpublish policy上の結果であって放送由来のProgram意味情報ではないため、解決済み・publish済みProgramについてもその否定値をprovider-dataへ保存しない。unresolved情報だけを根拠に現在candidateのONID / TSID / 物理情報を補完してChannel / Programを生成・更新せず、既存rowの失効・削除は通常のauthoritative snapshot契約だけに従う。

## provider-data 利用境界 / publish fingerprint

`Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` の具体schema、正規化、安定キー抽出、保存上限は `arib_si_engine_rs/DESIGN_JA.md` の「provider-data / 診断情報 Rust SSOT」と `arib_si_engine_rs/schema/*.schema.json` を正とする。TIS は保存schemaを再定義しない。

TIS Kotlin は provider-data JSON を `JSONObject.put()` や文字列連結で直接構築してはならない。TIS Kotlin は Rust JNI の build / 正規化 / key extraction API で得たbytesをTvProviderに書く。TIS が JNI へ渡す JSON は Rust builder への入力 DTO であり、TvProvider に保存する provider-data schema ではない。

Program provider-data の top-level envelope、必須フィールド、検証規則、正規化、安定キー抽出は TIS では再定義しない。正本は `arib_si_engine_rs/DESIGN_JA.md`、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json`、`arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json` とする。TIS instrumentation テスト用の期待値 JSON を置く場合は Rust 側テストデータとバイト単位で同一に保つ。

EIT文字列をTvProviderへ投影する際、TISはARIBの異なるlanguage codeを1つのtitle/descriptionへ連結しない。Rust snapshotが返す`shortEvents[] / extendedTexts[] / extendedItems[]`から`ARIB_SI_EPG_TvProvider投影方針.md`の単一言語選択規則に従って標準列用文字列を選び、候補列はprovider-data builderへ渡す。受信番組名が空の場合も`event-<eventId>`等の架空titleを生成しない。

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
// Kotlin facade。実JNIはclosed JSON result envelopeを返す。
object ProviderDataBridge {
    fun buildProgramProviderData(program: ProgramRecord): ProviderDataResult
    fun normalizeProgramProviderData(rawBytes: ByteArray): ProviderDataResult
    fun extractProgramKeyResult(rawBytes: ByteArray): ProgramKeyResult?
    fun buildChannelProviderData(channel: ChannelRecord): ProviderDataResult
    fun decodeChannelProviderData(rawBytes: ByteArray): ChannelProviderDataResult?
}

sealed interface ProviderDataResult
data class Success(
    val bytes: ByteArray,
    val schemaVersion: Int,
    val truncated: Boolean,
    val diagnosticsDroppedCount: Int,
) : ProviderDataResult
data class Failure(
    val errorCode: String,
    val errorMessage: String,
    val schemaVersion: Int,
) : ProviderDataResult

data class ChannelProviderDataResult(
    val canonicalBytes: ByteArray,
    val schemaVersion: Int,
    val serviceKey: ServiceKey,
    val tune: ChannelTune,
    val requiresCas: Boolean,
)
```

Rust JNIのclosed envelopeは`arib_si_engine_rs/DESIGN_JA.md`を正とし、facadeはfield集合・型・成功/失敗の整合を検査してSuccess/Failureへ変換する。入力不正・正規化失敗をIllegalStateExceptionに変換しない。現行requestのCAS根拠欠落もFailureとし、TvProviderWriterは当該serviceのchannel/program書込み・削除を開始せずerrorCode/errorMessageを診断へ渡す。失敗したprepared publicationのfingerprintはnull、commit対象サービスから除外し、Direct Boot完了根拠にしない。

`ChannelTune` は `deliverySystem`、`frequencyHz`、`streamIdType`、`streamId`、`physicalChannel`、`satelliteBand`、`remoteControlKeyId` だけを持つtyped物理tune復元値とし、`inputId`、表示名、backend名、driver名、px4相対slot等を持たない。channelとTvInputServiceの関連付けはchannel rowのrequired fieldである`TvContract.Channels.COLUMN_INPUT_ID`を唯一のSSOTとする。tune復元前にrowの`COLUMN_INPUT_ID`がcurrent TISの`TvInputInfo.id`と一致することを検証し、不一致rowのprovider-dataを別inputの物理tuneとして使用しない。`decodeChannelProviderData()` は invalid UTF-8、malformed JSON、schema不整合を null または診断付き失敗へ落とす。現行String JNI surfaceではtyped resultを単一JSON envelopeで返し、Kotlinはこのresult envelopeだけを読む。保存済みprovider-data自体の解釈・修復やTAB/hexの第二wire protocolは設けない。

`inputJson` は Rust builder への入力 DTO であり、TvProvider に保存する provider-data schema ではない。最終JSONバイト列、正規化、安定キー抽出はRustが行う。provider-data単体のdigestまたはsignatureは返さない。

`rawBytes` は任意バイナリではなく、既存 TvProvider に保存済みの JSON v1 UTF-8 バイト列を指す。Kotlin は `String(rawBytes)` などで再解釈してから Rust へ渡してはならず、TvProvider から取得した `COLUMN_INTERNAL_PROVIDER_DATA` の BLOB バイト列をそのまま Rust JNI 境界へ渡す。TvProvider が文字列として返した場合の互換補助は、UTF-8 バイト列へ戻すだけに限定し、Kotlin側でJSON構造を解釈・再構築してはならない。

`normalizeProgramProviderData(rawBytes)`、`extractProgramKey(rawBytes)`、`decodeChannelProviderData(rawBytes)`は、invalid UTF-8またはmalformed JSONをKotlin側で修復しない。Rustは診断付き失敗、key抽出失敗、またはchannel decode失敗へ落とし、通常実行経路で例外やpanicに変換しない。provider-data bytesだけのdigest APIと`ProviderDataResult.signature` / `contentDigest`は設けない。

### 診断情報 schema

Descriptor診断の機械検証規則は `arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json` を正とする。TIS は `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` 配下のオブジェクトを別 schema へ変換せず、Rust JNI が返した provider-data JSON 内の診断情報を保存する。ARIB視聴年齢制限は`ratings[]`にraw構造化値を残し、Android対応可否や写像結果はprovider-dataへ戻さない。TIS Kotlin は descriptor diagnostic JSON を独自生成しない。

### provider-data 保存上限

provider-data の soft limit / hard limit、診断情報・長文補助情報の切り詰め規則、切り詰め時の診断 key は `arib_si_engine_rs/DESIGN_JA.md` と Rust provider-data 実装を正とする。TISは保存前にRust JNIが返したbytesをそのまま扱い、Kotlin側で独自の切り詰めschemaを定義しない。

### SectionEvent 入力上限

TIS の PSI/SI section path は allocation 前に `SectionEvent.dataLength` を検証する。`MAX_SECTION_BYTES` は 4096 bytes とし、`dataLength` が 1..4096 の範囲にある場合だけ ByteArray 確保と `AribSiEngine` / CAS / Program publish への投入を許可する。section read size 不一致、0 length、負値相当、4096 bytes 超過は parser に渡さず診断カウンターに記録する。

### transaction DTO API

`AribSiEngine` 呼び出し側は複数 snapshot を合成してはならない。本番経路は以下の用途別bulk DTOを使う。engineから受け取るpolicy入力は`ServiceSemanticFacts`・event・EIT instanceの放送/受信事実であり、`ProgramPublishability`等のTIS product policyをRust側DTOに持たせない。

```kotlin
data class ExcludedEventDescriptorFacts(
    val serviceKey: ServiceKey,
    val stableIdentity: String?,
    val eventId: Int,
    val source: AribProgramSource,
    val descriptors: AribEventDescriptors,
)

data class ProgramPublishSnapshot(
    val discoveryProfile: Int,
    val authoritativeProgramKeysByService: Map<ServiceKey, Set<String>> = emptyMap(),
    val ingestSequence: Long,
    val events: List<AribEvent>,
    val updateWindows: List<EpgUpdateWindow>,
    val semanticFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts>,
    val descriptorDiagnostics: List<DescriptorDiagnostic>,
    val parserDiagnostics: List<ParserDiagnostic>,
    val malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int> = emptyMap(),
    val eitInstances: List<EitInstanceState> = emptyList(),
    val excludedEventDescriptorFacts: List<ExcludedEventDescriptorFacts> = emptyList(),
)

fun takeProgramPublishSnapshot(): ProgramPublishSnapshot
```

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
    val semanticFactsByServiceKey: Map<ServiceKey, ServiceSemanticFacts>,
    val diagnostics: List<ParserDiagnostic>,
    val eitInstances: List<EitInstanceState> = emptyList(),
)

fun serviceRegistrationSnapshot(): ServiceRegistrationSnapshot
```

```kotlin
data class CasDiscoverySnapshot(
    val services: List<AribService>,
    val caMetadata: List<CaMetadata>,
    val pmtPids: Map<ServiceKey, TsPid>,
    val catEmmPids: List<TsPid>,
    val diagnostics: List<DescriptorDiagnostic>,
    val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic> = emptyList(),
)

fun casDiscoverySnapshot(): CasDiscoverySnapshot
```

`ingestSequence`はsection ingestにより意味stateが更新された順序であり、snapshot read回数ではない。readするたびに増える`snapshotGeneration`は設けない。discovery stage、table requirement status、services、CA、diagnosticsは一回取得した同じimmutable native transactionから用途別DTOへ投影し、stageやCAS用serviceを別JNI readで再取得しない。

`MalformedCaDescriptorDiagnostic` は、少なくとも `pid`、`tableId`、`tableIdExtension`、`serviceId`、`elementaryPid`、`scope`、`offset`、`declaredLength`、`actualRemainingLength`、`reason`、`rawPrefixHex` を持つ。詳細診断の一次保存先は CAS discovery snapshot とし、Program provider-data は `malformedCaDescriptorCount` summary だけを保存する。

`takeProgramPublishSnapshot()`と`programStateSnapshot()`は、同じロック内で一回取得したimmutable native transactionからevents / EIT instance / service semantic facts / 診断情報を読み、同じKotlin policyでupdateWindowsを投影する。区間queueのdrainは行わない。公開経路は前者、LiveSessionの現在番組判定・視聴年齢制限判定・映像メタデータ補完は後者を使う。`snapshotEvents()`と`takeEpgUpdateWindows()`を別々に呼んで合成する経路は設けない。

`events`は公開policyを通過した候補だけとし、除外eventの完全な記述子事実は`excludedEventDescriptorFacts`へ保持する。この診断専用DTOは`AribEvent`ではなく、MapperのProgram入力へ渡さない。`descriptors.diagnostics.descriptorFactsCanonicalJson`はRustの構造化事実をそのまま保持し、不正parental descriptorの全raw bytes・entries・parse statusを64-byte診断prefixへ置き換えない。公開可否を再判定する第二policyや、診断専用の再parseは設けない。

eventの宣言descriptor loop長がsection残量を超える場合も、CRCを除く受信済みloop範囲を共通Rust parserで解析し、境界内で読める記述子事実を保持する。`descriptors.diagnostics.truncatedDescriptorLoop`はその場合だけ`AribTruncatedDescriptorLoop(declaredLength: Int, rawBytesHex: String, parseStatus: String)`を持つ。rawBytesHexは受信済みloop全体、parseStatusは`TruncatedDescriptor`とし、未受信bytesを補完しない。この診断専用情報から公開・削除権限を復元しない。

通常bulkの`programKey` / `stableIdentity`とKotlin `AribEvent` / `ExcludedEventDescriptorFacts`のstableIdentityは、`DEFINED` / `UNDEFINED_TIME`だけに値を持ち、それ以外はnullとする。raw eventIdとServiceKey、記述子事実はキー不在でも保持する。MapperはキーがないeventをProgramへ昇格させず、raw eventIdから補完しない。

廃止 snapshot wrapper は本番経路・公開通常境界・product build に残してはならない。テスト専用に必要な入口は test source または test-only 可視性に隔離し、本番 APK / JNI API / release API から参照不能にする。

### LiveSession / PlaybackPipeline / Scan の直列化

非同期 `MediaCodec.Callback.onError()` は現行codec identityと再生generationが一致する場合だけ扱う。AOSP `CodecException` の `ERROR_RECLAIMED` は必ず解放し、回復不能なエラーも旧codecを再利用しない。回復可能なエラーでは既存の全再生generation終了・再生成を使い、transientの場合は100ms後、それ以外のrecoverableの場合は次のexecutor処理で再生成する。自動再生成は外部からの一回のstart要求につき一回までとし、再失敗は音声なら既存のvideo-only新generationへの移行（audio-onlyは再生不能）、映像なら再生不能通知と全generation終了へ渡す。待機中のstop・retune・releaseで再生成予約を無効にする。codec単体の独立した回復state machineや無限の再取得loopは設けない。この回数と待機時間はプロダクトの回復方針であり、CDD/ARIBが規定する値とは扱わない。

codec回復時の旧generation終了・再生成予約・予約後の再生成は同じ失敗通知契約を使う。資源解放や再生成が例外終了した場合、または予約できない場合は、回復開始時の元generationを付けた`PLAYBACK_RECOVERY_FAILED`をSessionへ通知する。停止後の新しいgenerationへ付け替えない。Sessionは世代照合後、映像・音声のどちらを起点とした回復でも全再生の失敗として既存のFailed状態へ遷移し、字幕世代を失効させて再生不能を通知する。ResourceCleanupが保持する未解放資源は後続のstop/releaseで再試行し、解放失敗時は再生成予約へ進まない。別の回復所有者や追加の自動再試行は設けない。

この例外境界はfatal・ERROR_RECLAIMED・自動回復済みの再失敗による打切りにも適用し、音声のvideo-only移行中の解放例外を含め、元generation付きの全再生失敗を通知する。映像の打切りは停止成功後に元generationへ再生不能を通知し、停止例外時は共通の失敗通知へ渡す。回復回数や未解放資源の所有は変更しない。

MediaSync音声エラー、AudioTrack初期化失敗、音声decoderの入力・出力失敗・期限切れからの音声失敗処理と、音声出力先・PCM形式変更による全再生generation再生成も同じ例外境界を使用する。codec callbackだけの失敗理由ではないため、全再生の復旧失敗理由は`PLAYBACK_RECOVERY_FAILED`に統一する。audio-onlyは全generation停止成功後に元generationへ音声再生不能を通知し、停止例外時は共通の全再生失敗を一回通知する。AVでvideo-only再生成に必要な文脈を失った場合も、音声だけの失敗として無視せず全再生失敗を通知する。

再生成が例外を投げず失敗StartResultを返す場合は、Sessionの再生成結果受理経路が外部通知を所有する。元generationを照合して開始結果を既存状態へ反映し、Failedを確定し字幕世代を失効させた後に`notifyVideoUnavailable(UNKNOWN)`を呼ぶ。video filterとaudio-only filterの開始失敗、video-only移行の失敗もこの経路を使う。新generationを受理する前に届いた開始途中の通知だけへ依存しない。成功結果と古い元generationの結果ではこの失敗通知を行わない。

通常の再生開始と音声トラック切替の同期要求では、開始中の例外を発行済みgeneration付き失敗結果へ変換し、呼出元Sessionが同じ状態確定・字幕失効・再生不能通知を行う。失敗例外でStartingや旧Startedを残さない。Failed確定済みgenerationへ遅延した開始途中の失敗通知が届いても、外部通知を重複させない。開始前の入力拒否は従来どおりgeneration未発行として旧状態を維持し、未解放資源は既存所有者に残して後続のstop/releaseで再試行する。

AudioTrackの生成、音量・dual-mono設定、MediaSyncへの接続、routing listener登録は一つの初期化として扱い、全て成功した後だけ再生用AudioTrackを確定する。途中例外では生成済みtrackと部分登録listenerを既存cleanup所有者へ渡し、output-format callbackの例外境界から音声失敗処理へ進む。旧generationを終了してAVならvideo-only新generation、audio-onlyなら再生不能へ遷移し、解放失敗は既存ResourceCleanupに保持する。未接続のAudioTrackでcallback処理を継続しない。

AV・字幕・文字スーパーのFilterも、取得直後から設定・開始を同じ初期化処理で囲む。設定値の構築、configure、startの途中例外と失敗戻り値では、当該Filterのcallback受理用参照を先に外し、停止・解放を試行する。解放失敗は既存ResourceCleanupへ保持し、部分初期化したFilterを未所有のまま失わない。AudioTrackとFilterは同じ準備・確定・巻戻し処理を使用し、資源の所有者を追加しない。

`MaleicacidLiveSession` は session-level serial executor を持ち、currentサービス、generation、track state、unblock state、latest videoメタデータ、`ProgramPublishCoordinator`へのアクセスを同一executorに閉じる。AV開始lifecycleはSessionが`Idle / Starting(signature) / WaitingFirstOutput(signature,generation) / Started(signature,generation) / Failed(signature,generation?) / Stopped`のsealed stateを一つだけ所有する。current/pending signature、last attempted/started gate、pipeline generationを並行して保持しない。遷移判定は状態を持たない純粋関数とする。TunerController、PlaybackPipeline、parental receiverのコールバックは直接state mutationせず、session executorにenqueueする。

`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。filter、block model decoder、MediaSync、MediaSync input Surface、AudioTrack、generation、surface、未返却audio buffer id、availability arm sequenceの変更を呼び出し元スレッドで直接行わない。release後のqueued taskはreleased flagとgenerationで破棄する。

TIFの`Session.onSetStreamVolume(volume)`は各Live sessionが所有する相対音量要求であり、初期値を`1.0f`、受付範囲を`0.0f..1.0f`とする。範囲外は同区間へclampし、system/master volumeや他sessionの音量を変更しない。Session executorで保持した値をPlaybackPipeline executorへ渡し、現在のAudioTrackへ適用する。音声track切替、decoder再起動、video-only fallbackからの復帰などでAudioTrackを再生成するときも、そのsessionが保持する最新値を新しいAudioTrackへ一度適用してからMediaSyncへ接続する。音声経路が未生成の時点の要求も捨てず、次回生成時に適用する。

`ChannelScanManager` は`ActiveScanTask(generation, purpose, context, cancelRequested, controller, engine)`を一つのatomic referenceとして所有する。running boolean、active generation/purpose、controller、engine、contextを別fieldに複製しない。cancel / cleanup taskは取得した同じtask identityにだけ作用し、stale cleanupが後続scanを変更してはならない。Tuner Framework/TRMへ渡すpriority hintは`ScanPurpose`から全列挙で一意に決め、setup scanを`PRIORITY_HINT_USE_CASE_TYPE_SCAN`、boot EPG同期とbackground maintenanceを`PRIORITY_HINT_USE_CASE_TYPE_BACKGROUND`、liveを`PRIORITY_HINT_USE_CASE_TYPE_LIVE`とする。frontend等のhardware arbitrationは再実装しない。一方、ライブ中はboot/background作業の開始を延期するというTIS製品policyだけはManagerに残す。

scan用TunerにもTuner Framework標準のresource-lost listenerを登録し、callbackのtune generationがそのscan candidateのactive generationと一致する場合だけ当該taskを`RESOURCE_LOST`で終端する。TunerControllerはresource-lost受付時に既存のtuneAcceptedをfalse、currentTuneをnullにして、caption言語・時刻のlogical状態も失効させる。以後の同世代section配送と重複lost通知は拒否する。CAS接続・descrambler解放は本書「CAS / descrambler の現行境界」の世代付きtransactionと単一所有契約に従い、scanのdynamic filter更新待機後にもlost通知を確認する。その後、再生停止、section filter解放、CAS解放、各caption parser解放を全件試行し、成否にかかわらずlost generationを上位callbackへ一度通知する。未解放filter/caption parserは既存の所有mapへ残して後続cleanupで再試行する。primary/suppressedの失敗は通知後に診断し、controller executorと選局失効を維持する。ChannelScanControllerは既存のactive/lost generationと公開lockをResourceLossFenceへ集約し、通知後のSI snapshot取得とTvProvider publishを拒否して残りcandidateへ進まない。収集ループ終了時のfilter再解放も失敗した場合は失敗を診断へ残し、終端理由RESOURCE_LOSTを一般例外や成功へ上書きしない。資源喪失を観測していない収集失敗は従来どおり伝播する。遅延した旧generationのresource-lostは後続candidate/taskへ伝播させない。setup scanは上位へ失敗を返し、利用者または正規setup flowの再要求を待つ。boot EPG同期はpendingを維持して既存schedulerへ再試行を返し、background maintenanceは失敗終了して次の既存scheduleに委ねる。TIS独自の優先度、resource pool、即時再取得loopは追加しない。

BS事前stream-ID探索も一つのStreamIdDiscoveryOperationにgeneration・結果・待機完了を所有する。Tuner.scanとcallback・資源喪失・cancelは既存controller executorへ直列化し、latch待機だけを呼出元で行う。`onLocked()`を受けたときは同じsettings / `SCAN_TYPE_AUTO`の`Tuner.scan()`を正確に一度再発行してHALのscan継続契約へ進み、`onScanStopped()`までは結果を確定しない。重複`onLocked()`は再発行しない。明示tune前でtuneAcceptedがfalseでも探索中のresource-lostを受け付け、待機を解除してRESOURCE_LOSTをResourceLossFenceへ渡す。喪失後のstream-ID報告、候補展開、後続explicit tune、SI収集・公開を拒否する。cancel失敗でも喪失結果を別理由へ書き換えず、未解放operationは既存ownerに残してreset/closeで再試行する。古い待機の終了は新operationをcancelしない。探索の受信結果はSCANNINGから一度だけ確定する。正常停止はonScanStoppedのみで確定し、onProgressの100%では待機を解除しない。停止・timeout・開始失敗・取消し・喪失で結果確定後は遅延IDを拒否する。明示取消しはnative cancelのSUCCESS後だけ確定し、非SUCCESS/例外ではownerを保持する。受信結果と未解放ownerへの喪失通知は別の寿命とし、停止・timeout後もownerが残る間のresource-lostを上位fenceへ一度だけ通知する。確定済み受信結果は書き換えず、scan全体の喪失終端と公開拒否はResourceLossFenceを優先する。

BS探索の開始失敗後もTuner SDK内にscan callbackが登録済みの場合があるため、cancelScanningによる後処理を試行する。後処理が非SUCCESSまたは例外でもSTART_FAILEDのresultCode/messageを上位へ返し、後処理失敗は診断と既存ownerに保持する。次のreset/closeで解放を再試行し、解放が成功するまで新しい探索を開始しない。開始失敗だけを根拠にownerを解放済みとしない。

再選局前のresetは旧currentTuneとtuneAccepted、caption/clockのlogical stateを先に失効させ、その後playback停止・tune listener解除・section filter・CAS・caption parserの解放を全件試行する。失敗は集約して上位へ返し、その呼出しでは新しいscan/tuneを発行しない。新tuneがSUCCESSでも初期section filter群の準備完了まではtuneAcceptedを公開せず、準備失敗時は同じresetでrollbackする。失敗した準備世代は再使用せず、遅延section/tune callbackを受理しない。

解放失敗時の所有は上位まで維持する。TunerControllerが所有CASのcloseも担当し、scan/liveから同じCASを重複closeしない。ChannelScanManagerはcontroller/engineの解放を全件試行し、成功した参照だけをnullにする。未解放ActiveScanTaskはclosingとして保持し、全解放成功後だけactiveTaskから除去する。解放失敗は診断と所有参照に残し、確定済みRESOURCE_LOST / Cancelled / Completed等のsemantic終端理由を上書きしない。解放再試行の成功も終端理由を変更しない。engine生成後はcontroller構築より先にtaskへ所有を登録する。boot同期の解放失敗はpendingと再試行要求を維持する。

既存PlaybackResourceCleanupの実装をResourceCleanupへ共通化し、playbackとlive teardownが同じ「失敗した解放actionだけを保持して再試行する」処理を使用する。liveは解放要求後の通常操作を拒否し、closeが失敗した資源だけを再試行する。ChannelScanManagerのlive session集合が未解放sessionを所有し、全解放成功後だけ登録を除去してsession executorを停止する。onRelease再呼出しと、既存のscan/live受付契機からManager executorへ投入する解放再試行を用い、独立scheduler・即時再取得loopは追加しない。解放待ちのscanは後続scanを受け付けず、解放待ちliveはboot/background scanの受付を開かない。

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

Channel provider-data の新規書き込み・読み取り正形式は JSON v1 のみとする。`key=value;...` 形式、旧 flat provider-data、旧 provider-data 断片は読み取り互換入力としても残さない。JSON v1 は `schema="maleicacid.tv.channel"` / `schemaVersion=1` を持ち、保存項目と正規化は`arib_si_engine_rs/DESIGN_JA.md`とRust provider-data APIを正とする。表示名の正本は`Channels.COLUMN_DISPLAY_NAME`とし、provider-dataへ重複保存しない。`inputId`はprovider-dataへ重複保存せず、channel rowのrequired `TvContract.Channels.COLUMN_INPUT_ID`をSSOTとする。`channelRegistrationReady`、`epgPublishable`、`unsupportedCas`、`clearLivePlaybackSupported`等のTIS policyを保存しない。

### 旧 indexed JNI / 廃止経路の禁止

TIS は `nativeSnapshotBulkJson()` と provider-data JNI API を通常境界とする。`nativeGetEventCount()`、`nativeGetEvent*` indexed JNI getter、旧 event JSON `canonicalGenres` フィールド、互換専用の空返却シンボル、未使用 private external 宣言は残してはならない。旧経路を使う呼び出し不能コードや test-only 以外の廃止予定 path は、互換維持ではなく削除する。

### Program publish retry

再試行の所有者、key、entry、現在の更新区間による再検証、cooldown、容量、破棄条件は本書「TvProvider failure semantics」を唯一の正本とする。本節に独立したqueue契約を定義しない。

Provider 必須問い合わせ failure、Program insert/update failure、廃止行削除 failure、publish fingerprint build failureではpublish fingerprint cache更新と `DirectBootEpgPending`解除に進まない。廃止行削除は `deletionAuthoritative=true` の更新区間でのみ実行する。

### AttributionSource

LineageOS 22.1の通常経路では、`TvInputService.onCreateSession(inputId, sessionId, tvAppAttributionSource)`で受け取ったnon-null `tvAppAttributionSource`をsession寿命中のattribution正本とする。session生成時に`serviceContext.createContext(new ContextParams.Builder().setNextAttributionSource(tvAppAttributionSource).build())`で変更不能なsession固有`sessionContext`を作り、`sessionId`、`tvAppAttributionSource`、`sessionContext`を同じsession creation snapshotへ確定する。途中失敗ではSessionを公開せず、作成済みartifactを解放する。

Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。AudioTrack生成はAndroid 15（API 35）環境の公開`AudioTrack.Builder.setContext(sessionContext)`を必須とし、`sessionContext.getAttributionSource()`からTV app attribution chainとdevice固有audio session情報を伝播させる。通常経路で素の`serviceContext`をAudioTrackへ渡さず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。生成したAudioTrackは同generationのMediaSyncへ設定し、session releaseまたは置換後は旧`sessionContext`と旧AudioTrackを新しいMediaSync generationへ再利用しない。

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

AVCのPMT記述子とESのSPSが共にある場合はprofile_idc・constraint flags・level_idcの一致を要求する。PMT記述子がない場合はSPSの値を能力照合に使い、放送記述子があったとは扱わない。不正・矛盾した記述子、Androidへ変換できないprofile/level、必要なMediaFormatに対応するdecoderがない場合は`UNSUPPORTED_VIDEO_STREAM`とする。`MediaCodecList.findDecoderForFormat`へprofile/levelと寸法を渡し、返された名前でdecoderを生成する。音声も必要なsample rate・channel countを能力照合へ渡す。この照合成功は部分ESのブロック入力、実機の初回表示、機器別budgetの実測認定を代替しない。

| codec | 追加認識時の扱い |
|---|---|
| MPEG-2 Video | 必須対応。PMT / component descriptor から codec、解像度、走査方式、aspect を認識し、MediaFormat、block model decoder起動、MediaSync first-frame gate、unsupported診断情報を固定する。 |
| H.264 / MPEG-4 AVC | 必須対応。profile / level は AVC video descriptor と実 MediaCodec capability を照合し、未対応時は codec unsupported 診断に落とす。 |
| H.265 / HEVC | codec として認識対象。r51はmetadata / 診断へ保持する。r52では現行対象の従来TS profileでARIB signaling上HEVCが現れる場合をgeneric direct playback selectionへ含め、MediaFormat / block model decoder / MediaSync first-output gate / unsupported診断まで必須とする。 |

ISO/IEC 14496-2 Visual、JPEG 2000、auxiliary video、SVC、MVC、3D additional view は、今回の ISDB-T/S product scope のライブviewable codecとして対応宣言しない。必要ならprovider-data / 診断情報にARIB signalingを保持する。

### audio codec

下表のcodec認識と実再生の適合証拠を区別する。現行HALの有限audio extractorはADTSとMPEG audioのframe境界・時刻を処理し、LATM/LOASの抽出・時刻付与を実証した経路ではない。LATM/LOASというsignalingやAIDL enumを認識すること、または端末にdecoderが存在することだけで、その形式の再生対応を宣言しない。再生の証明には形式ごとにHALの境界・PTS、TISの構成とblock入力、実decoder/AudioTrack出力まで対応する証拠が必要である。ADTS/ASC/PCEのhost試験を含め、現在の証拠だけでは機器別の再生適合は未検証とする。

ADTSの構成は有限header probeから読み、LCのobject type、周波数index、channel_configurationを検査する。ASCとADTS/PCEの構文解析は`arib_si_engine_rs`のstatelessな共通部品`codec_signaling`へ集約し、TISで構文処理を複製しない。JNIへ渡すstartup入力は既存AAC probe予算の64 KiB以内、PMTのASCは記述子のsize欄の255 bytes以内とし、構成不正と構成待ちを区別する。PMTにASCがある場合はADTSの周波数・channel_configurationを照合し、元のASC bytesを`csd-0`へ渡す。HE-AACの明示SBRは拡張周波数を使い、放送profileをMediaCodec能力照合へ渡す。ASC先頭がAOT=2であることだけを根拠に後続の暗黙SBR/PSが無いとは断定しない。

channel_configuration=7は8ch、0はPCEを構成根拠にする。PMTのASCに有効なPCEがあればそのchannel countを使う。帯域内PCEの場合はraw data block先頭のPCEを読み、SCE/CPE/LFEの参照からchannel countを算出し、PCE fieldと元commentをASC基準のbyte alignmentへ移して`csd-0`を構成する。PCEのprofile・周波数の不一致、要素参照の重複、完全に受信したPCEの長さ不正は構成不正とする。PCEがまだない場合は1chへ推測せず、既存の有限startup予算・期限の範囲で待つ。PCEのないframeは宣言長で送るが、queue用のframe/AUの再構成やpayloadコピーは行わない。HE-AAC-v2の入力設定は現行再生対象に含めない。構成probeのhost検証だけをdecoder実機適合の合格根拠にしない。

| codec | 追加認識時の扱い |
|---|---|
| MPEG-2 AAC | 必須対応。ADTS / MPEG-2 AAC LC、channel count、sample rate、ISO639 language、main/sub、dual mono、音声モード、音質表示を保持する。 |
| MPEG-2 BC Audio | 認識対象。decoder が利用できる場合だけ再生対応を対応宣言し、未対応時はvideo-only診断に落とす。 |
| MPEG-4 AAC / HE-AAC | 必須認識。AAC LC / HE-AAC profile、LATM/LOAS / ADTS、AudioSpecificConfig、channel count、sample rate を保持する。decoder が利用できる場合だけ再生対応を対応宣言する。 |
| MPEG-4 ALS | codec として認識対象。対象 transport profile を本プロダクトが対応宣言しない場合はplayable capabilityに入れない。対応する場合は block model decoder / MediaSync / AudioTrack / メタデータ / unsupported診断情報まで必須。 |

MPEG-H 3D Audio と AC-4 は ARIB STD-B32 4.0以降の改定概要で高度地上デジタルテレビジョン放送向け追加codecであることを確認できるが、STD-B79 / STD-B80 の高度地上方式は本productの恒久scope外であるためcodec固定表には含めない。AC-3、Enhanced AC-3、DTS、DTS-HD、Dolby TrueHDも現行対象transportに対する取得可能なARIB本文の条項根拠を確認せず推測で追加しない。

## provider-data 受け渡し境界（推奨案A）

TIS は TvProvider 標準列への投影を担当する。TIS は `Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` に保存される最終 JSON を直接生成してはならない。

TIS が JNI へ渡す JSON は、保存形式ではなく Rust へ値を渡すための受け渡し用形式である。この受け渡し用形式の型、必須項目、欠落時の扱い、旧形式拒否、値域検査は Rust の serde 型を正とする。TIS はこの受け渡し用 JSON を provider-data schema の Kotlin 実装、保存形式または正規形として扱ってはならない。

受け渡し用形式の schema 名は `maleicacid.tv.programRequest` / `maleicacid.tv.channelRequest` とし、保存用 schema 名 `maleicacid.tv.program` / `maleicacid.tv.channel` を名乗らない。

Rust は受け渡し用 JSON を serde 型へ読み込み、検査し、保存用JSON、識別子、切り詰め診断を生成する。TIS は Rustが返した保存用JSONをそのままTvProviderの`internal_provider_data`に保存する。TISはRustが返した識別子と診断結果だけを使う。TIS runtimeのpolicy結果、track identity、Android投影結果はbuilder入力へ含めず、保存JSONへ戻さない。

TIS は保存データの型、正規化、必須項目判定、欠落補完、旧形式互換、識別子抽出、サイズ上限処理を実装してはならない。TIS 側で `0`、`false`、`jpn`、`UNKNOWN`、空文字などを使って必須項目欠落を補い、provider-data を成立させてはならない。

`DescriptorDiagnosticV1` は Rust が生成した正規 JSON を正とする。TIS は `DescriptorDiagnosticV1` を項目ごとに再構築してはならない。TIS が保持する場合は、Rust 生成の正規 JSON を不透明な文字列として透過保持する。

TIS の試験は、受け渡し用 JSON の細部を保存形式として検査しない。検査対象は Rust provider-data builder が返した保存用JSON、識別子、拒否診断に寄せる。
