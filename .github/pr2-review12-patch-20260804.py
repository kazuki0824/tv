from pathlib import Path
import re
import sys

root = Path(sys.argv[1]).resolve()
tis_path = root / 'tis/DESIGN_JA.md'
hal2_path = root / 'tuner_hal2/DESIGN_JA.md'

def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{label}: expected 1 match, got {count}')
    return text.replace(old, new, 1)

def regex_once(text: str, pattern: str, repl: str, label: str) -> str:
    new_text, count = re.subn(pattern, repl, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f'{label}: expected 1 match, got {count}')
    return new_text

tis = tis_path.read_text(encoding='utf-8')

# 3. CS110: internal None/null, omit both builder setters, normalize the Android 14 default at HAL.
old = '''CS110 tune request 生成時、TIS は Android Tuner API builder の default `streamId` / `streamIdType` に依存しない。CS110 では frontend stream selector を明示的に none / `UNDEFINED` 相当に設定する。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BSの通常製品経路はIF周波数と`STREAM_ID`のTSIDを使う。TISはdriver固有slotへ変換せず、typed selectorの検証とbackend ABIへの写像はTuner HALへ委ねる。
'''
new = '''CS110のTIS内部モデルとTvProvider保存形式では、frontend stream selectorを`None`／`null`として保持する。Android 14 Tuner API builderへ変換するときは`streamId`と`streamIdType`のsetterをどちらも呼ばない。builderが生成する`STREAM_ID`と`INVALID_STREAM_ID(0xFFFF)`の組を、Tuner HALが公開契約境界で`NoSelector`へ正規化する。TISから`UNDEFINED`、0、TSID、relative番号を「selectorなし」の代用として明示設定しない。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BSの通常製品経路はIF周波数と`STREAM_ID`のTSIDを使う。TISはdriver固有slotへ変換せず、typed selectorの検証とbackend ABIへの写像はTuner HALへ委ねる。
'''
tis = replace_once(tis, old, new, 'CS110 selector contract')

# 4. Audio-only service classification belongs to TIS and is not a signal/decoder failure.
old = '''現行対応 video ES が存在しない サービスは viewable としない。HEVC など 現行未対応 video codec のみを持つ サービスは、再生不能として `notifyVideoUnavailable()` へ落とす。HEVC などを codecメタデータとして追加認識する場合でも、ISDB-S3 / MMT / TLV 等の恒久対象外 transport profile 由来の場合は ライブ viewable capability として 対応宣言しない。現行仕様でも、PMT上で認識できる未対応 codecメタデータは provider-data の `components.video[]` / `components.audio[]` に `currentPlaybackSupported=false`、`liveViewableClaim=false`、`diagnosticCode=UNSUPPORTED_CODEC` として保存し、再生可能表明とは分離する。

現行対応 video ES が存在し、audio ES が存在しない、または audio codec だけが 現行未対応の場合は、video-only サービスとして視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。AC-3 / Enhanced AC-3 / MPEG-H 3D Audio は、今回確認した ARIB 資料群では国内放送全般の対象 codec として固定する根拠を持たないため、codec 固定表には含めない。
'''
new = '''現行製品が登録対象とするARIB `service_type`は、`0x01`のdigital television serviceと`0x02`のdigital radio sound serviceに固定する。`0x01`は`TvContract.Channels.SERVICE_TYPE_AUDIO_VIDEO`、`0x02`は`TvContract.Channels.SERVICE_TYPE_AUDIO`へ写像する。その他のservice typeは壊れたサービスへ丸めず、`UNSUPPORTED_SERVICE_TYPE`を記録して現行製品スコープ外としてchannel登録しない。対応集合を追加する場合は、ARIB上の意味、TvProvider写像、PMT成立条件、再生経路を同じ変更で追加する。

`service_type=0x02`は本来的なaudio-only serviceである。少なくとも1本の現行対応audio ESと物理選局情報、`ServiceKey`、inputId、表示名が揃えば、video ESを要求せず`SERVICE_TYPE_AUDIO`として登録し、audio filter・decoder・AudioTrackだけを開始する。視聴sessionでは映像filterを開かず、サービス分類確定後に`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY)`を通知し、audio再生の成否と映像なし通知を分離する。audio codec非対応またはaudio ES欠落は`AUDIO_ONLY`の正常理由ではなく、`UNSUPPORTED_AUDIO_CODEC`または`SERVICE_TYPE_PMT_MISMATCH`として再生不能にする。

`service_type=0x01`はaudio-video serviceであり、現行対応video ESがない場合にaudio-onlyへ再分類しない。弱信号またはlock喪失は`VIDEO_UNAVAILABLE_REASON_WEAK_SIGNAL`、有効なserviceでdecoder起動またはqueue補充を一時待機する場合だけ`VIDEO_UNAVAILABLE_REASON_BUFFERING`、video codec非対応またはPMT構成不整合は`VIDEO_UNAVAILABLE_REASON_UNKNOWN`と型付き診断`UNSUPPORTED_VIDEO_CODEC`／`SERVICE_TYPE_PMT_MISMATCH`へ分離する。HEVCなど未対応codecのmetadataはprovider-dataへ保存してよいが、再生可能表明には使わない。

現行対応 video ES が存在し、audio ES が存在しない、または audio codec だけが 現行未対応の場合は、video-only サービスとして視聴可能にする。この場合、`notifyVideoUnavailable()` には落とさず、`audio absent` または `unsupported audio codec` を診断に残す。未対応 audio codec は、対応 video が存在する限り video-only 診断の対象であり、video unavailable の直接理由にしてはならない。AC-3 / Enhanced AC-3 / MPEG-H 3D Audio は、今回確認した ARIB 資料群では国内放送全般の対象 codec として固定する根拠を持たないため、codec 固定表には含めない。
'''
tis = replace_once(tis, old, new, 'audio-only service contract')

# 1. Break the decoder/filter startup cycle with a finite pre-start startup budget.
pattern = re.escape('decoder は PMT の stream_type だけでは構成せず、MediaEvent payload から MPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio header を検出してから MediaFormat を構成する。') + r'.*?' + re.escape('正の有限値を確定できないdecoder/profileは開始しない。')
repl = '''PMTからcodec family、audio/video種別、PIDを確定した後、AV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。snapshotは製品profileで事前検証した有限値として、`singleEventLimitBytes`、`startupQueueBudgetBytes`、`startupQueueMaxSamples`、`startupQueueMaxDurationUs`、`pendingQueueBudgetBytes`、`pendingQueueMaxSamples`、`pendingQueueMaxDurationUs`、`decoderStartupDeadlineMs`、`steadyBackpressureDeadlineMs`を持つ。codec headerをまだ受信していないこと、またはdecoderが未構成であることを理由に値を動的導出しない。全codec共通の8 MiB、4 sample、1000 msへ固定せず、対象codec、対象decoder/device組合せ、最大access unit、header収集量、reorder depth、allocator上限、実機最悪値からofflineで検証する。正の有限値と必要領域を開始前に予約できないprofileはAV filterを開始しない。

startup queueと台帳claimを確保した後にAV filterを開始し、上限内の`MediaEvent`からMPEG-2 sequence header、H.264 SPS/PPS、AAC ADTS/AudioSpecificConfig、MPEG audio headerを収集して`MediaFormat`を構成する。decoder構成成功後は同じsnapshotのsteady-state上限へ遷移し、startup queueの保持分を通常queueまたはdecoderへ所有権移管する。runtimeで観測したdecoder input buffer/block capacityは各sampleの投入可否と製品profile検証の診断にだけ用い、開始済み世代のsnapshotまたは予約量を書き換えない。検証済み最小容量を満たさないdecoderではfilterを停止し、claimとHAL handleを解放して`DECODER_CAPACITY_MISMATCH`を記録し、`notifyVideoUnavailable(VIDEO_UNAVAILABLE_REASON_UNKNOWN)`へ進む。

MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= mapped buffer capacity`を満たす場合だけstartup queueまたは通常decoder queueへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。'''
tis = regex_once(tis, pattern, repl, 'playback startup order')

# 7. Backoff timestamps are eligibility thresholds, not guaranteed wake-up times.
old = '''Program publish retry queue は現行仕様では process-local とする。process death 後の retry 永続化は行わず、boot/background scan による再収集を正とする。ただし、process-local queue であっても retry key は `ServiceKey + updateWindow + failureClass`、entry は `attempt / nextAttemptAtMillis / firstFailureAtMillis / lastFailureAtMillis` を持ち、1/5/15/60分 backoff、決定的 jitter ±20%、最大10回、24時間 retention を適用する。
'''
new = '''Program publish retry queue は現行仕様では process-local とする。process death 後の retry 永続化は行わず、boot/background scan による再収集を正とする。ただし、process-local queue であっても retry key は `ServiceKey + updateWindow + failureClass`、entry は `attempt / earliestEligibleAtMillis / firstFailureAtMillis / lastFailureAtMillis` を持つ。1/5/15/60分 backoffと決定的jitter ±20%は、次回実行可能になる最短時刻`earliestEligibleAtMillis`の算出にだけ使い、その時刻でのwake-upまたは実行開始を保証しない。最大10回、24時間 retention を適用する。
'''
tis = replace_once(tis, old, new, 'retry eligibility field')
old = '''次回 `publishLiveProgramsForCurrentService()`、boot EPG sync、background maintenance の publish entrypoint 先頭で retry queue を drain する。成功した key は削除し、失敗した key は保持する。process restart では retry queue を破棄し、boot/background sync による再収集を正とする。provider failure 時は 廃止行削除、publish fingerprint更新、pending平文 に進まない。
'''
new = '''次回 `publishLiveProgramsForCurrentService()`、boot EPG sync、background maintenance の publish entrypoint 先頭で、`now >= earliestEligibleAtMillis`のentryだけを実行対象としてdrainする。未到達entryはqueueに保持し、entrypointが来ない限り指定時刻でのwake-upは行わない。成功した key は削除し、失敗した key はattemptを進めて新しい`earliestEligibleAtMillis`を設定する。process restart では retry queue を破棄し、boot/background sync による再収集を正とする。provider failure 時は 廃止行削除、publish fingerprint更新、`DirectBootEpgPending`解除 に進まない。
'''
tis = replace_once(tis, old, new, 'retry drain semantics')

# 8. Replace undefined pending wording with a formal Direct Boot state.
old = '''boot EPG sync の pending 平文 は boot EPG sync task 単位で判定する。task 中に cancel request を観測した場合は、成功 candidate があっても pending を 平文 しない。成功 candidate は `collectSiForCandidate()` が `COMPLETE` で、かつ 登録可能サービスが1件以上存在する candidate とする。background maintenance は pending 平文 を扱わない。
'''
new = '''Direct Boot保留の正式状態を`DirectBootEpgPending`とする。`BootEpgSyncCoordinator`がinputIdごとにdevice-protected storage上のこの状態を所有し、boot EPG sync要求を受理した時点または未完了・失敗終了時に設定する。状態はprocess restartとuser unlockをまたいで保持し、background maintenanceは設定・解除しない。

同一boot EPG sync taskがcancelされず、`collectSiForCandidate()`が`COMPLETE`となるcandidateを1件以上得て、登録対象channel／Programに必要なTvProvider必須問い合わせとinsert/update/deleteが一つのpublish transactionとして全て成功したcommit後にだけ`DirectBootEpgPending`を解除する。provider query/write failure、publish fingerprint生成失敗、cancel、登録可能サービス0件では保留を維持する。candidate成功だけ、部分write、またはfingerprint cache更新だけを解除根拠にしない。
'''
tis = replace_once(tis, old, new, 'Direct Boot pending state')
# Normalize remaining legacy wording.
tis = tis.replace('Direct Boot保留解除', '`DirectBootEpgPending`解除')
tis = tis.replace('pending平文', '`DirectBootEpgPending`解除')
tis = tis.replace('pending 平文', '`DirectBootEpgPending`解除')

# 6. Android 14 exposes AudioTrack.Builder.setContext(Context); reflection is not a normal path.
old = '''`AttributionSource?` は `TvInputService.onCreateSession(..., AttributionSource)` から `MaleicacidLiveSession`、`TunerController`、`PlaybackPipeline` へ保持して渡す。対象 Android の `AudioTrack.Builder` が `setAttributionSource(AttributionSource)` を公開している場合は reflection を使わない直接呼び出しへ移行する。Android 14 system SDK 境界では compile visibility 差があるため、`PlaybackPipeline` は reflection による補助設定を行い、失敗時は警告 ログ に残して audio usage/content type/session 設定を継続する。
'''
new = '''`AttributionSource?` は `TvInputService.onCreateSession(..., AttributionSource)` から、Tuner SDKなど型付きAPIが要求する境界まで保持して渡す。AudioTrack生成ではAndroid 14（API 34）の公開`AudioTrack.Builder.setContext(Context)`へ`TvInputService`のnon-null `Context`を直接設定し、ContextからAttributionSourceとdevice固有audio session情報を伝播させる。`setAttributionSource()`を探索・呼出しするreflection、hidden API呼出し、reflection失敗時の無言fallbackを通常経路に置かない。対象system APIを使う必要が生じた場合は、対象SDKへ直接コンパイルできる型付き呼出しとして別途設計する。
'''
tis = replace_once(tis, old, new, 'AudioTrack attribution')

# Sanity checks for TIS changes.
for stale in (
    'decoder構成完了後かつAV filter開始前',
    'nextAttemptAtMillis',
    'pending 平文',
    'pending平文',
    'reflection による補助設定',
    '明示的に none / `UNDEFINED` 相当に設定',
):
    if stale in tis:
        raise SystemExit(f'stale TIS text remains: {stale}')
for required in (
    'startupQueueBudgetBytes',
    'SERVICE_TYPE_AUDIO',
    'earliestEligibleAtMillis',
    'DirectBootEpgPending',
    'AudioTrack.Builder.setContext(Context)',
    'setterをどちらも呼ばない',
):
    if required not in tis:
        raise SystemExit(f'missing TIS text: {required}')

tis_path.write_text(tis, encoding='utf-8')

# 5. Distinguish static query rules from the selected dynamic status snapshot model.
hal2 = hal2_path.read_text(encoding='utf-8')
old = '''参照系メソッドは、サービス調停が同一lock内で不変snapshotを作り、AIDL境界が応答へ変換する。参照処理は状態変更、後片付け、ワーカー停止、callback配送を行わない。
'''
new = '''静的inventory／capability参照メソッドは、サービス調停が同一lock内で変更不能な`CapabilitySnapshot`から応答snapshotを作り、AIDL境界が応答へ変換する。動的な`IFrontend.getStatus()`／`getFrontendStatusReadiness()`は`../tuner_hal/DESIGN_JA.md`の世代付き`FrontendStatusSnapshot`契約を正とし、現行製品ではtune/scan workerまたはbackend監視ownerがbounded backend I/O完了後に更新した値を読む。参照呼出し自身は状態変更、後片付け、ワーカー停止、callback配送を行わない。AOSPはqueryごとの同期backend readを必須にしていないため、現在のsnapshot方式を維持する。将来、特定statusをbounded同期readへ変更する場合は、対象status、I/O上限、失敗写像、generation再検証、snapshot更新との排他を公開状態表へ追加してから有効化する。
'''
hal2 = replace_once(hal2, old, new, 'query model')
old = '- read-only queryからcleanup、worker操作、backend I/Oを開始しない。'
new = '- 静的inventory／capability queryからcleanup、worker操作、backend I/Oを開始しない。動的frontend status queryは現行製品では世代付き`FrontendStatusSnapshot`だけを読み、query呼出しを契機にbackend I/Oを開始しない。'
hal2 = replace_once(hal2, old, new, 'query prohibition')
for required in ('静的inventory／capability参照メソッド', 'FrontendStatusSnapshot', 'bounded同期read'):
    if required not in hal2:
        raise SystemExit(f'missing HAL2 text: {required}')
hal2_path.write_text(hal2, encoding='utf-8')
