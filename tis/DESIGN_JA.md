# TIS 設計判断

## AOSP 標準経路

TIS は `TvInputService` として システムTVアプリ から呼ばれ、Tuner HAL には Tuner SDK API 経由でアクセスする。HAL binder を直接呼ばない。
TIS の setup / boot EPG sync / user unlock drain は、固定文字列や package 名を inputId とみなしてはならない。`TvInputManager.tvInputList` から自 `MaleicacidTvInputService` に一致する `TvInputInfo.id` を一意に解決し、その inputId だけを scan / sync / TvProvider writer へ渡す。解決不能または複数一致の場合、boot EPG sync は pending のまま延期し、setup scan は開始しない。

## BS と CS110 の選局契約

BSはIF周波数とAOSP Tuner公開契約のtyped stream selectorを保持する。通常のscan候補、channel保存、再選局ではbackend種別に依存せず、`STREAM_ID`のTSID `0..65534`だけを使用する。TISはpx4の相対slot、Linux DVBの`DTV_STREAM_ID`、HAL内部のbackend capabilityを取得・推測・保存しない。CS110は周波数帯だけでscan candidateとtune selectorを作り、stream selectorを保存しない。

CS110のTIS内部モデルとTvProvider保存形式では、frontend stream selectorを`None`／`null`として保持する。Android 14 Tuner API builderへ変換するときは`streamId`と`streamIdType`のsetterをどちらも呼ばない。builderが生成する`STREAM_ID`と`INVALID_STREAM_ID(0xFFFF)`の組を、Tuner HALが公開契約境界で`NoSelector`へ正規化する。TISから`UNDEFINED`、0、TSID、relative番号を「selectorなし」の代用として明示設定しない。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BSの通常製品経路はIF周波数と`STREAM_ID`のTSIDを使う。TISはdriver固有slotへ変換せず、typed selectorの検証とbackend ABIへの写像はTuner HALへ委ねる。

TvProvider の channel internal provider data には JSON v1 `tune.streamIdType` と `tune.streamId` を保存する。通常製品経路で書き込む値は、`NONE` の `streamId=null`、または `TSID` の `0..65534`だけとする。`65535`はAOSP `INVALID_STREAM_ID`であり、実TSIDとして保存または再投入しない。`RELATIVE`はdriver固有値になるため、TISの通常channelデータへ保存しない。


## 製品 scan 候補表の保持者

TIS は製品 scan 候補表の実装データ保持者である。選局対象、対象周波数帯、BS/CS110 selector 境界、CATV 候補範囲の設計契約は tv 直下の開発規則.mdを正とする。

TIS が保持する候補表の具体値は製品 scan 実装データのSSOTである。ただし、選局対象範囲、VHF除外、CATV C13〜C63限定、BS/CS110 selector 境界などの設計契約は tv 直下の `開発規則.md` を正とする。TIS の候補表は `開発規則.md` の設計契約に反してはならない。

TIS 以外の文書や実装に同等の scan 候補表を重複保持してはならない。Tuner HAL に渡す値は、TIS が生成した explicit tune candidate に限定する。

TIS は地上UHF、CATV、BS、CS110の候補を持ち、Tuner HALには explicit tune candidate として渡す。Tuner HAL は日本向け scan 候補表を自前生成しない。

製品scan候補の対象範囲、CATV C13〜C63の中心周波数、VHF除外、BSの通常selector規範値はtv直下の`開発規則.md`の「製品 scan 候補の規範値」を唯一の設計正本とする。本書は、その規範値をTISが実装データとして保持しexplicit tune candidateを生成する責務だけを定義し、同じ規範値を再定義しない。


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

現行 product の平文 non-tunneled AV入力は、Tuner `MediaEvent.getLinearBlock()` と `MediaCodec` block model の型付き経路だけを正式経路とする。video／audio decoderは `MediaCodec.CONFIGURE_FLAG_USE_BLOCK_MODEL` で構成し、入力slotの `MediaCodec.QueueRequest.setLinearBlock(linearBlock, offset, dataLength)` にPTSとflagsを設定してqueueする。reflection、hidden API、`LinearBlock.map()`でESを`ByteArray`へ複製して通常input bufferへ入れる経路、通常ByteBuffer input modelへの代替処理を禁止する。`MediaEvent`、`LinearBlock`、decoder input claimはqueue成功または破棄確定まで保持し、queue成功後にだけ呼出側所有権を解放する。secure `MediaEvent`は現行平文productの対象外とし、mappable blockへの暗黙変換を行わない。

`getLinearBlock()`がnull、block model configureまたはQueueRequestが利用不能、offset／lengthがblock範囲外、decoderが当該blockを受理しない場合は`BLOCK_MODEL_UNAVAILABLE`または入力不正の型付き診断へ落とす。成功を偽装せず、現generationのfilter、未queue event、decoder、MediaSync、startup queue、budget claimを解放して`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。

デコード後のA/V同期とSurface提示はAndroid標準`MediaSync`だけを使用する。video decoder出力は`MediaSync.setSurface(sessionSurface)`後の`createInputSurface()`へ接続し、output frameをPTSナノ秒でreleaseする。audio decoder出力PCMは`MediaSync.queueAudio()`へPTS付きで渡し、`MediaSync.Callback.onAudioBufferConsumed()`を受けるまでaudio decoderのoutput buffer IDとPCM `ByteBuffer`（block model出力では`OutputFrame`／output `LinearBlock`を含む）を解放・変更・再利用しない。圧縮入力側の`MediaEvent`／input `LinearBlock`は`QueueRequest.queue()`成功時にcodecへ所有権を移管し、PCM消費完了までは保持しない。独自media clock、`AudioTimestamp`を使う独自同期、独自future render／late drop schedulerを設けない。

`notifyVideoAvailable()`は`Filter.start()`成功、汎用`FilterEvent`、payload付き`MediaEvent`到着だけでは呼ばない。現generationのvideo decoder outputがMediaSync input Surfaceへ時刻付きでrenderされ、session Surfaceが有効で、視聴制限でblockされず、同generationでMediaSync Surface errorが発生していないことをgateとする。これは公開APIで確認可能なMediaSync入力到達を表し、最終display pixelの厳密なpresent証明とはみなさない。

## EIT と TvProvider

現行 product の EIT 収集は EIT p/f を主経路とする。EIT schedule actual `0x50..0x5F` は、scan/setup 後に `TvProvider.Programs` へ最低限の初期番組情報を出すための短期補完に限って利用する。schedule actual を常時収集または長期 EPG 収集として扱わない。EIT schedule other `0x60..0x6F`、長期 schedule window、サービス横断 EPG 更新、予約録画・追従録画向けの高度利用は現行 product 対象外とする。Programs の `internal_provider_data` には JSON v1 の stable `programKey`、timing、CAS state、長形式イベント項目、component/audio メタデータ、series 完全構造、イベントグループ `relatedItems`、linkage、free_CA_mode、音声言語、レーティング、診断 JSON を TIS 内部データとして保存する。TvProvider の標準 column には title / short description / long description、broadcast genre、明示写像できる canonical genre、series id、episode display number、item count、scrambled、audio language、コンテンツレーティング など自然対応できる範囲だけ反映する。

TvProvider標準列への投影判断は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とする。`internal_provider_data` の schema、canonical encode、保存上限、診断 schema は `arib_si_engine_rs/DESIGN_JA.md` と Rust serde 構造体を正とする。本書は TIS runtime における取得、書き込み契機、retry、現在番組解決、視聴セッションでの利用だけを定義する。

## 字幕表示の責務

ARIB 字幕は TIS 側の字幕 path で `libaribcaption` を使用する。現行 product では PMT から字幕 track を検出し、`TvTrackInfo.TYPE_SUBTITLE` として通知し、`onSetCaptionEnabled()` と字幕表示経路を接続する。字幕 track を advertise する場合は、ARIB 字幕 PES を libaribcaption C API 経路で処理し、実際に表示できることを対応宣言条件に含める。`arib_si_engine_rs` の自前 ARIB 文字列 decoder はサービス名・番組名・番組説明など字幕以外の SI/EPG 文字列に限定し、字幕 PES や字幕本文をその decoder に渡さない。libaribcaption は C API のみを使用し、独自 C/C++ 薄層 は書かない。Kotlin から直接 C API を呼ばず、TIS Kotlin → Rust JNI boundary → 安全なRustラッパー → libaribcaption C API の順に接続する。BML / data broadcast 実行環境、双方向データ放送 UI、データ放送 UI は恒久対象外である。

現行製品profileの字幕取得は、PMTで字幕ESを検出した場合だけ`TYPE_TS / SUBTYPE_PES`を開き、字幕PIDと明示`streamId=0xBD`（`private_stream_1`）で設定する。STD-B24 6.4-E1 Fascicle 1の9.1.1、9.2、9.3、9.5、9.6を独立PES字幕、data group、PTS、PMT descriptorの根拠とし、STD-B32 3.11-E1 Fascicle 3の3.1を`private_stream_1=0xBD`と宣言長付きPESの根拠とする。これはTIS字幕経路が選ぶ利用設定であり、Tuner HALのPES capabilityを`0xBD`へ制限する契約ではない。HAL正本は有効な明示`streamId 0..255`、wildcard `0xFFFF`、映像`0xE0..0xEF`の長さ0 PESを同じ広告済みPES能力で受理する。現行TISは字幕取得でwildcard、別stream ID、長さ0映像PESを要求しないが、それらをHAL非対応と推定または再定義してはならない。一般PESを利用するTIS機能を追加する場合は、同じ公開HAL契約をそのまま使用する。


## libaribcaption Soong / renderer 統合境界

ARIB字幕表示は、repoで供給される `libaribcaption-android` の product fork を Soong build graph に入れ、renderer 有効の `libaribcaption.so` として生成したものだけを正式経路とする。out-of-graph の `.so`、renderer 無効 build、`dlopen()` 確認だけ、decoder API 呼び出しだけ、Canvas 文字描画だけを 字幕対応宣言条件にしてはならない。

`libmaleicacid_arib_caption_jni` は `libaribcaption` に明示依存し、`MaleicacidTvInput` は JNI library として `libmaleicacid_arib_caption_jni` を取り込む。TIS は字幕 PES を Rust JNI boundary と安全な Rust ラッパー経由で libaribcaption C API に渡し、renderer 出力を字幕 overlay へ接続する。字幕 PES を受け取っても renderer 表示に到達できない状態を字幕対応成功として扱ってはならない。

## ライブ playback 実装方式

TIS のライブplaybackは、Tuner AV filterの平文`MediaEvent.LinearBlock`をMediaCodec block modelへcopyなしで投入し、video decoder出力を`MediaSync.createInputSurface()`へ、audio decoder出力PCMを`MediaSync.queueAudio()`へ渡す標準経路だけを採用する。MediaSyncはsession SurfaceとAudioTrackを所有し、A/V同期、映像提示時刻、音声clock追従を担当する。TISはMediaSyncの外側に独自clockまたは独自frame schedulerを置かない。

`tunneled`／platform passthrough playback pathは現行productの設計候補から外し、実装しない。`notifyVideoAvailable()`は現generationの最初のdecoded video frameがMediaSync input Surfaceへ時刻付きでrenderされたことをgateとし、単なるfilter開始または入力event到着をgateにしない。

setup scan の channel registration は global discovery complete を必須条件にしない。ただし partial snapshot を無条件に channel insert に使ってはならない。TvProvider 登録には サービス単位の登録可能 gate を使う。登録可能 は、ONID / TSID / SID が確定し、channel URI から物理 tune key に戻せ、PMT PID と PMT、PCR PID、現行対応 video ES が取得済みで、audio は対応済みまたは video-only として診断可能であり、サービス名 は正式名または deterministic な仮名と後続更新方針を持ち、平文ライブ視聴の対応宣言可能 または scrambled unsupported として状態通知可能な サービスに限定する。登録可能未満の partial snapshot は 診断情報 / ライブ更新 / debugにのみ使い、channel insert しない。scrambled サービスは channel 登録してよいが、CAS 仮実装 のまま 平文ライブ視聴成功 対応宣言 してはならない。

## codec header / A-V sync / publish mode の固定

ライブ playback の codec 構成は、現行 product では video は MPEG-2 video と H.264/AVC、audio は AAC と MPEG audio を対象 codec とする。国内放送全般であり得る codec を追加で扱う場合は、対象 codec、MediaFormat、decoder 起動、AudioTrack、first-frame gate、unsupported 診断情報の契約を設計正本へ固定してから扱う。

現行製品が登録対象とするARIB `service_type`は、`0x01`のdigital television serviceと`0x02`のdigital radio sound serviceに固定する。`0x01`は`TvContract.Channels.SERVICE_TYPE_AUDIO_VIDEO`、`0x02`は`TvContract.Channels.SERVICE_TYPE_AUDIO`へ写像する。その他のservice typeは壊れたサービスへ丸めず、`UNSUPPORTED_SERVICE_TYPE`を記録して現行製品スコープ外としてchannel登録しない。対応集合を追加する場合は、ARIB上の意味、TvProvider写像、PMT成立条件、再生経路を同じ変更で追加する。

`service_type=0x02`は本来的なaudio-only serviceである。少なくとも1本の現行対応audio ESと物理選局情報、`ServiceKey`、inputId、表示名が揃えば、video ESを要求せず`SERVICE_TYPE_AUDIO`として登録し、audio filter・decoder・AudioTrackだけを開始する。視聴sessionでは映像filterを開かず、サービス分類確定後に`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)`を通知し、audio再生の成否と映像なし通知を分離する。audio codec非対応またはaudio ES欠落は`AUDIO_ONLY`の正常理由ではなく、`UNSUPPORTED_AUDIO_CODEC`または`SERVICE_TYPE_PMT_MISMATCH`として再生不能にする。

`service_type=0x01`はaudio-video serviceであり、現行対応video ESがない場合にaudio-onlyへ再分類しない。弱信号またはlock喪失は`VIDEO_UNAVAILABLE_REASON_WEAK_SIGNAL`、有効なserviceでdecoder起動またはqueue補充を一時待機する場合だけ`VIDEO_UNAVAILABLE_REASON_BUFFERING`、video codec非対応またはPMT構成不整合は`VIDEO_UNAVAILABLE_REASON_UNKNOWN`と型付き診断`UNSUPPORTED_VIDEO_CODEC`／`SERVICE_TYPE_PMT_MISMATCH`へ分離する。HEVCなど未対応codecのmetadataはprovider-dataへ保存してよいが、再生可能表明には使わない。

現行対応 video ES が存在し、audio ES が存在しない、または audio codec だけが 現行未対応の場合は、video-only サービスとして視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。AC-3 / Enhanced AC-3 / MPEG-H 3D Audio は、今回確認した ARIB 資料群では国内放送全般の対象 codec として固定する根拠を持たないため、codec 固定表には含めない。

PMTからcodec family、audio/video種別、PIDを確定した後、AV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。snapshotは製品profileで事前検証した有限値として、`singleEventLimitBytes`、`startupQueueBudgetBytes`、`startupQueueMaxSamples`、`startupQueueMaxDurationUs`、`pendingQueueBudgetBytes`、`pendingQueueMaxSamples`、`pendingQueueMaxDurationUs`、`decoderStartupDeadlineMs`、`steadyBackpressureDeadlineMs`を持つ。codec headerをまだ受信していないこと、またはdecoderが未構成であることを理由に値を動的導出しない。全codec共通の8 MiB、4 sample、1000 msへ固定せず、対象codec、対象decoder/device組合せ、最大access unit、header収集量、reorder depth、allocator上限、実機最悪値からofflineで検証する。正の有限値と必要領域を開始前に予約できないprofileはAV filterを開始しない。

startup queueと台帳claimを確保した後にAV filterを開始し、上限内の`MediaEvent`からMPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio headerを収集して`MediaFormat`を構成する。header解析に必要な最小範囲だけ`LinearBlock.map()`でread-only参照してよいが、ES本体を`ByteArray`へ複製して通常input bufferへ移送してはならない。decoder構成成功後は同じsnapshotのsteady-state上限へ遷移し、startup queueの`MediaEvent`／`LinearBlock`所有権をblock model QueueRequestへ移す。runtimeで観測したdecoder block capacityは各sampleの投入可否と製品profile検証の診断にだけ用い、開始済み世代のsnapshotまたは予約量を書き換えない。検証済み最小容量を満たさないdecoderではfilterを停止し、claimとHAL handleを解放して`DECODER_CAPACITY_MISMATCH`を記録し、`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。

MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= LinearBlock capacity`を満たす場合だけstartup queueまたはblock model QueueRequestへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。

必要なqueue領域とclaim台帳はplayback generation開始時に原子的に予約する。各eventはrange検証後、block model投入前に`dataLength`をsnapshot台帳へclaimし、いずれかのevent、byte、sample、duration上限を超える場合は原因別に`SAMPLE_TOO_LARGE`または`PENDING_QUEUE_FULL`を記録してHAL handleを解放する。claim済みbyte、sample、durationはQueueRequest成功、破棄、generation変更、stop、releaseで正確に返す。HALの`avPerFilterLiveBytes`または`avRuntimeBudgetBytes`をTISへ公開・複製・1event上限化しない。

first frame前はcodec-specificな`decoderStartupDeadlineMs`を用い、必要なsequence header、SPS/PPS、audio config、reorder用入力を収集している間の一時queue増加を通常backpressure失敗へ写像しない。startup deadlineまでにdecoder入力可能状態またはfirst frameへ到達できず、queueのbyteまたはduration上限も解消しない場合だけplaybackを停止して`notifyVideoUnavailable()`へ進む。first frame後は別の`steadyBackpressureDeadlineMs`を用い、単発超過は当該sampleを解放して継続し、期限中にdequeue進行がなくqueue上限が継続する場合だけunavailableへ遷移する。audioだけの超過はvideo-only継続可否を既存規則で判定し、無条件にvideo unavailableへ写像しない。

A/V同期方式は現行productでAndroid標準`MediaSync`に固定する。tunneled playbackと`avSyncHwId`はTIS non-tunneled playback範囲外であり、TISは使用しない。`PlaybackPipeline`のserial executorが、現generationのMediaSync、MediaSync input Surface、session Surface、AudioTrack、video／audio decoder、未返却audio buffer id、playback rateを単一所有する。decoder callback、MediaSync callback、AudioTrack／route callbackはstateを直接変更せず、同executorへ直列化する。

videoは`MediaSync.setSurface(sessionSurface)`の後に`MediaSync.createInputSurface()`を一度だけ呼び、そのSurfaceをvideo decoder出力先とする。decoded outputは元PTSをナノ秒へ変換してMediaSync input Surfaceへrenderする。TISは`AudioPlaybackClock`、`StandalonePlaybackClock`、`VideoFrameScheduler`、`AudioTimestamp.framePosition`由来の独自media position、独自future frame保持、独自late drop閾値、独自renderTimestamp算出を実装しない。

audioはsession固有Contextで作った`AudioTrack`を`MediaSync.setAudioTrack()`へ設定し、audio decoderの現generation output PCMをPTS付き`MediaSync.queueAudio()`へ渡す。block model audio outputの`OutputFrame.getLinearBlock()`は必要範囲をmapし、返されたByteBufferをMediaSyncへ渡す。MediaSyncから`onAudioBufferConsumed(sync, buffer, bufferId)`が返るまで、該当codec output index、OutputFrame、LinearBlock、ByteBuffer、budget claimを保持し、変更・再利用・releaseしない。callback後に対応するcodec outputを非描画releaseし、所有権とclaimを返す。

MediaSyncは生成時のplayback rate 0を用いて必要な有限prefillを行い、視聴制限gate、Surface有効性、decoder開始、最小startup条件成立後に`PlaybackParams`のspeed 1.0で開始する。video-onlyではAudioTrackを設定せずMediaSync video経路を使い、audio-onlyではSurfaceとvideo decoderを設定せずMediaSync audio経路を使う。MediaSync errorは`MEDIASYNC_ERROR_SURFACE_FAIL`と`MEDIASYNC_ERROR_AUDIOTRACK_FAIL`を区別し、surface失敗はvideo unavailableへ、audio失敗は既存video-only継続規則へ写像する。

retune、playback generation変更、stop、flush、非wrap PTS discontinuity、Surface変更、AudioTrack切替／再生成、audio route変更、decoder再生成では、既存MediaSyncの内部anchorを再利用せず、playback rateを0へ戻して未返却audio bufferと旧decoder outputを回収し、MediaSync input Surface、MediaSync、decoder、AudioTrackを解放して新generationとして再生成する。通常33-bit PTS wrapはgeneration内でunwrapし、wrapだけでは再生成しない。旧generationのdecoder／MediaSync／route callbackはstate更新に使わず、旧bufferを非描画解放する。

最低試験契約は、型付きLinearBlock→block model QueueRequest、ES全体copy禁止、video PTSのMediaSync input Surface到達、audio bufferのconsume callbackまでの寿命、A/V同期、video-only、audio-only、通常PTS wrap、非wrap discontinuity後のMediaSync再生成、Surface／route変更後の旧generation非利用、MediaSync error写像、stale generation非描画を含む。試験のqueue数値上限は選択した`ProductProfile`と一致させる。

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
- 現行の video 対応宣言対象は MPEG-2 video `0x02` と H.264/AVC `0x1b` に限定する。HEVC `0x24` は 現行平文ライブ視聴 / playback selection 対象外であり、診断上は `NO_SUPPORTED_VIDEO_ES` 相当として扱う。HEVC などを国内放送全般であり得る video codec として認識対象に追加する場合は、codec 固定表と playback selection 境界を設計正本へ固定してから扱う。
- ARIB 視聴年齢制限 は Android `TvContentRating` へ `domain=com.android.tv`, `ratingSystem=ISDB`, `rating=ISDB_<age>` として写像する。対応範囲は JPN かつ レーティング 4..20 のみとし、未対応 country / レーティングは推測変換せず `internal_provider_data` / 診断に残す。レーティング 未取得時は `TvContentRating.UNRATED` として 視聴制限判定する。
- `notifyVideoAvailable()` は現generationのdecoder first-frame callbackがMediaSync input Surfaceへの時刻付きrenderを示し、session Surfaceが有効で、同generationのMediaSync Surface errorがなく、視聴制限でblockされていない場合だけ呼ぶ。旧generationのdecoder／MediaSync callbackは無視する。
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

現行の EIT 対象 table は、present/following actual `0x4E`、present/following other `0x4F`、および scan/setup 後の初期登録・短期補完に使う schedule actual `0x50..0x5F` のうち、上記対象 サービスに属する event とする。schedule actual を常時・長期収集として扱わない。schedule other `0x60..0x6F` は現行 Programs publish/delete 対象外であり、更新区間 を発生させない。

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

`selectedProgramId` のような TvProvider row id 依存値は publish fingerprintの構成要素にしてはならない。必要な場合は補助診断として扱い、publish skip判定を壊さない。


## 現在番組 選択

現在番組 resolver は TvProvider query 時点で `START_TIME_UTC_MILLIS <= now AND END_TIME_UTC_MILLIS > now` に絞る。sort order は `START_TIME_UTC_MILLIS DESC, END_TIME_UTC_MILLIS ASC, _ID DESC` に固定する。overlap がある場合も cursor 返却順には依存せず、この selection rule で1件を選ぶ。

provider-data 診断情報は `diagnostics.currentProgram` 配下に保存する。`selectionRule` は `START_DESC_END_ASC_ID_DESC` とする。対象なしの場合は empty string とする。`overlapCount` と `selectedProgramId` は補助診断として扱い、publish fingerprintの意味上のidentity にしない。ARIB `event_id` は `COLUMN_EVENT_ID` と JSON v1 `programKey.eventId` で扱う。

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
}

data class ProviderDataResult(
    val bytes: ByteArray,
    val schemaVersion: Int,
    val truncated: Boolean,
    val diagnosticsDroppedCount: Int,
)
```

`inputJson` は Rust builder への入力 DTO であり、TvProvider に保存する provider-data schema ではない。最終JSONバイト列、正規化、安定キー抽出はRustが行う。provider-data単体のdigestまたはsignatureは返さない。

`rawBytes` は任意バイナリではなく、既存 TvProvider に保存済みの JSON v1 UTF-8 バイト列を指す。Kotlin は `String(rawBytes)` などで再解釈してから Rust へ渡してはならず、TvProvider から取得した `COLUMN_INTERNAL_PROVIDER_DATA` の BLOB バイト列をそのまま Rust JNI 境界へ渡す。TvProvider が文字列として返した場合の互換補助は、UTF-8 バイト列へ戻すだけに限定し、Kotlin 側で JSON 構造を解釈・再構築してはならない。

`normalizeProgramProviderData(rawBytes)`、`extractProgramKey(rawBytes)`、`appendCurrentProgramDiagnostics(rawBytes, ...)`は、invalid UTF-8またはmalformed JSONをKotlin側で修復しない。Rustは診断付き失敗またはkey抽出失敗へ落とし、通常実行経路で例外やpanicに変換しない。provider-data bytesだけのdigest APIと`ProviderDataResult.signature` / `contentDigest`は設けない。

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
    // CAS 診断一次保存先から Program provider-data summary へ渡す service_id 別件数。
    // Program ごとに raw descriptor / table / PID context を重複展開しない。
    val malformedCaDescriptorCountByServiceId: Map<Int, Int>,
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

`PlaybackPipeline` は playback-level serial executor を持ち、`setSurface()`、`setVolume()`、`start()`、`switchAudio()`、`stop()`、`release()` の state mutation を同一 executor に閉じる。filter、block model decoder、MediaSync、MediaSync input Surface、AudioTrack、generation、surface、未返却audio buffer id、トークンの変更を呼び出し元スレッドで直接行わない。release後のqueued taskはreleased flagとgenerationで破棄する。

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
- `MediaEvent` sampleは固定4 MiBを上限にしない。負のoffset、0以下のlength、加算overflow、`offset + length > LinearBlock capacity`は不正入力として確保前に破棄する。正常sampleはES全体をcopyせず、同一製品profileのper-event予算をclaimしてblock model QueueRequestへ渡す。共有領域方式とイベント固有fd方式を同じpending byte予算へ計上する。
- decoder／MediaSync入力の逆圧は無通知破棄ではない。sampleまたは未返却audio outputは上限付きpending queueとbudget claimに保持し、後続callback／drainで再試行する。sampleを破棄するのは上限付きqueueが満杯の場合だけとし、破棄counterを加算する。

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

Tuner SDKのTRM接続にはframework由来`sessionId`を`Tuner(serviceContext, sessionId, useCase)`へ渡す。AudioTrack生成はAndroid 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`を必須とし、`sessionContext.getAttributionSource()`からTV app attribution chainとdevice固有audio session情報を伝播させる。通常経路で素の`serviceContext`をAudioTrackへ渡さず、2引数版／1引数版の互換経路から3引数通常経路へ黙示fallbackしない。生成したAudioTrackは同generationのMediaSyncへ設定し、session releaseまたは置換後は旧`sessionContext`と旧AudioTrackを新しいMediaSync generationへ再利用しない。

`setAttributionSource()`を探索・呼出しするreflection、hidden API、vendor独自AIDL、reflection失敗時の無言fallbackを通常経路に置かない。対象system APIを使う必要が生じた場合は、対象SDKへ直接コンパイルできる型付き呼出しとして別途設計する。


## 本プロダクト対象TSであり得るcodec固定表

ARIB 資料上の本プロダクト対象TSであり得る codec を追加認識対象にする場合は、次の固定表を設計正本に吸収してから扱う。ここでの「扱う」は、PMT / component descriptor / 音声コンポーネントdescriptor / stream type / codecメタデータを認識し、TvProvider / trackメタデータ / 診断情報へ正しく反映することを含む。ISDB-S3 / MMT / TLVは恒久対象外であり、それらだけに依存するcodecまたは音響構成を本表の根拠へ持ち込まない。

### 参照した ARIB 資料と根拠

| 根拠資料 | 本改訂で固定する内容 |
|---|---|
| ARIB STD-B32 3.11-E1 Fascicle 1 Chapter 3 3.1〜3.3 | 国内デジタル放送の映像符号化方式は MPEG-2 Video、MPEG-4 AVC、HEVC の3系統として扱う。 |
| ARIB STD-B32 3.11-E1 Fascicle 2 Chapter 3 3.1〜3.4、Chapter 5、Chapter 6 | 国内デジタル放送の音声符号化方式は MPEG-2 AAC、MPEG-2 BC、MPEG-4 AAC、MPEG-4 ALS の4系統として扱う。 |
| ARIB STD-B10 5.13-E1 Part 2 Table 6-5 / 6.2.26 / Annex E | MPEG-2 系映像、H.264/AVC、H.265/HEVC、MPEG-2 Audio、AAC ADTS、MPEG-4 Audio LATM の signaling を認識対象にする。 |

### video codec

| codec | 追加認識時の扱い |
|---|---|
| MPEG-2 Video | 必須対応。PMT / component descriptor から codec、解像度、走査方式、aspect を認識し、MediaFormat、block model decoder起動、MediaSync first-frame gate、unsupported 診断情報を固定する。 |
| H.264 / MPEG-4 AVC | 必須対応。profile / level は AVC video descriptor と実 MediaCodec capability を照合し、未対応時は codec unsupported 診断に落とす。 |
| H.265 / HEVC | codec として認識対象。対象 transport profile を本プロダクトが 対応宣言しない場合は ライブ viewable capability に入れない。対応する場合は MediaFormat / block model decoder / MediaSync first-frame gate まで必須。 |

ISO/IEC 14496-2 Visual、JPEG 2000、auxiliary video、SVC、MVC、3D additional view は、今回の ISDB-T/S product scope の ライブ viewable codec として 対応宣言しない。必要なら provider-data / 診断情報に保持する。

### audio codec

| codec | 追加認識時の扱い |
|---|---|
| MPEG-2 AAC | 必須対応。ADTS / MPEG-2 AAC LC、channel count、sample rate、ISO639 language、main/sub、dual mono、音声モード、音質表示を保持する。 |
| MPEG-2 BC Audio | 認識対象。decoder が利用できる場合だけ再生対応を 対応宣言 し、未対応時は video-only 診断に落とす。 |
| MPEG-4 AAC / HE-AAC | 必須認識。AAC LC / HE-AAC profile、LATM/LOAS / ADTS、AudioSpecificConfig、channel count、sample rate を保持する。decoder が利用できる場合だけ再生対応を 対応宣言する。 |
| MPEG-4 ALS | codec として認識対象。対象 transport profile を本プロダクトが 対応宣言しない場合は playable capability に入れない。対応する場合は block model decoder / MediaSync / AudioTrack / メタデータ / unsupported 診断情報 まで必須。 |

AC-3、Enhanced AC-3、MPEG-H 3D Audio、DTS、DTS-HD、Dolby TrueHD は、今回確認した ARIB 資料群では国内デジタル放送の対象 codec として固定する根拠を確認できないため、codec 固定表には含めない。

## provider-data 受け渡し境界（推奨案A）

TIS は TvProvider 標準列への投影を担当する。TIS は `Programs.COLUMN_INTERNAL_PROVIDER_DATA` / `Channels.COLUMN_INTERNAL_PROVIDER_DATA` に保存される最終 JSON を直接生成してはならない。

TIS が JNI へ渡す JSON は、保存形式ではなく Rust へ値を渡すための受け渡し用形式である。この受け渡し用形式の型、必須項目、欠落時の扱い、旧形式拒否、値域検査は Rust の serde 型を正とする。TIS はこの受け渡し用 JSON を provider-data schema の Kotlin 実装、保存形式または正規形として扱ってはならない。

受け渡し用形式の schema 名は `maleicacid.tv.programRequest` / `maleicacid.tv.channelRequest` とし、保存用 schema 名 `maleicacid.tv.program` / `maleicacid.tv.channel` を名乗らない。

Rust は受け渡し用 JSON を serde 型へ読み込み、検査し、保存用JSON、識別子、切り詰め診断を生成する。TIS は Rustが返した保存用JSONをそのままTvProviderの`internal_provider_data`に保存する。TISはRustが返した識別子と診断結果だけを使う。

TIS は保存データの型、正規化、必須項目判定、欠落補完、旧形式互換、識別子抽出、サイズ上限処理を実装してはならない。TIS 側で `0`、`false`、`jpn`、`UNKNOWN`、空文字などを使って必須項目欠落を補い、provider-data を成立させてはならない。

`DescriptorDiagnosticV1` は Rust が生成した正規 JSON を正とする。TIS は `DescriptorDiagnosticV1` を項目ごとに再構築してはならない。TIS が保持する場合は、Rust 生成の正規 JSON を不透明な文字列として透過保持する。

TIS の試験は、受け渡し用 JSON の細部を保存形式として検査しない。検査対象は Rust provider-data builder が返した保存用JSON、識別子、拒否診断に寄せる。
