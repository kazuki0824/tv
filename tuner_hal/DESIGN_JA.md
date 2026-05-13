# Tuner HAL 設計判断

## VTS / 実機ゲート対象

ISDB-T、BS、CS110 の explicit tune と clear live / DVR path をゲート対象とする。HAL は BLIND_SCAN や HAL-generated Japanese scan plan を claim しない。Tuner HAL は渡された tune request を処理する。  
日本向け scan 候補、サービス検出、channel key の実装データ保持者は TIS とし、設計契約は tv 直下の開発規則.mdに従う。

`config/tuner_vts_config_aidl_V2.xml` は explicit tune point、AV filter、record DVR path の接続確認に限定する。descrambler object は Tuner HAL AIDL 面として実装するが、CAS HAL placeholder のまま production descrambling success は claim しない。

r51 は TS-only HAL profile とする。IP / MMTP / TLV / ALP filter は claim せず、`IFilter.configureIpCid()` は filter 種別にかかわらず `UNAVAILABLE` とする。CID を保存だけして matching / routing / delivery に使わない成功 no-op を残してはならない。


## AIDL 契約境界

`IFilter`、`IDvr`、`IFrontend`、`IDemux` の public method は、AIDL HAL の契約面として close 後状態を必ず検査する。`close()` 自体は idempotent とし、二重 close は成功してよい。ただし close 済み object に対する `getQueueDesc()`、`configure()`、`start()`、`stop()`、`flush()`、`read()`、`write()`、`getAvSharedHandle()`、`releaseAvHandle()`、`setDataSource()`、DVR の `configure()` / `start()` / `stop()` / `flush()` / `setFileDescriptor()` などは成功扱いにしない。close 後の失敗は、Tuner HAL service-specific error の `INVALID_STATE` を基本とする。`getId()` のような識別子取得だけを例外にする場合は、その例外を実装・単体テスト・VTS期待値で明示する。

`IFrontend.getStatus(statusTypes)` は、要求された `statusTypes` の各要素に対して、同じ順序で1つの `FrontendStatus` を返す。未対応 status type を黙ってdropして短い配列を返してはならない。未対応 status type が要求された場合、`getStatus()` は呼び出し全体を `INVALID_ARGUMENT` として失敗させる。`getFrontendStatusReadiness(statusTypes)` は AOSP VTS 期待に合わせ、要求された全 status type と同じ長さの readiness 配列を返す。`statusCaps` 外の type は要素ごとに `UNSUPPORTED`、`statusCaps` 内で backend が現在利用不可または status word / telemetry を現在取得できない場合は `UNAVAILABLE`、tune/probe 中なら `UNSTABLE`、有効値を返せる状態なら `STABLE` とする。`statusCaps`、`getStatus()`、`getFrontendStatusReadiness()` は同一の status support 判定 SSOT を使うが、戻り方は API ごとの AOSP 契約に従って分ける。`statusCaps` には起動時列挙時点で値の取得根拠を固定できる status type だけを含め、read 時に失敗し得る optional ioctl 由来の status type は含めない。telemetry 未取得値を `0` として成功返却してはならない。


Android 14 の Tuner HAL AIDL Rust backend では、`IFilter.setDataSource()` の source filter が Rust generated trait 上 non-null `Strong<dyn IFilter>` として現れるため、AOSP Java / JNI / HIDL に存在する `setDataSource(null)` による demux source 復帰経路は r51 の Rust-only 実装対象に含めない。この構造課題は `future_work/not_planned/android14_aidl_rust_descrambler_pid_only_boundary_report.md` に、`IDescrambler.addPid()` / `removePid()` の null source filter / PID-only 境界と同根の Android 14 AIDL/Rust nullable filter 境界として同一ファイル内で管理する。r51 は non-null source filter linkage、demux default source、`configure()` による既存 上流接続 clear、close / runtime-failed / invalid linkage の error mapping を修正・確認対象にする。AOSP frozen/stable AIDL の vendor 独自改変、C++/NDK wrapper、raw Binder transaction parser による generated trait 迂回は採用しない。

`IFrontend.tune()` は binder thread 上で lock 完了まで待ち続けない。前回 tune / scan の worker を generation で無効化し、backend へ tune request を投入し、非同期 worker が lock timeout と event 通知を行う。`stopTune()`、`close()`、次回 `tune()`、`scan()` は該当 generation を cancel し、古い worker からの `LOCKED` / `NO_SIGNAL` 通知を捨てる。

`IFrontend.close()` は frontend backend の critical cleanup を成功扱いで握り潰さない。public close では、scan cancel、tune worker stop、live pump stop、backend close、callback clear、demux unbind、frontend lease release を step runner として扱い、途中 step が失敗しても後続 cleanup を継続し、最初に観測した critical error を AIDL status として返す。cleanup failure 後の frontend object は通常操作へ戻さず、close retry または Drop 補助 cleanup だけを許可する。補助経路では失敗を返せないため、失敗を成功扱いにせず runtime diagnostic に残す。

DVB / earth_pt1 backend では、`DTV_CLEAR` は明示的な tune 停止操作である `stop_tune()` の責務とする。DVB backend の `close()` は reader stop と fd release を行うが、`DTV_CLEAR` の実行を close の必須条件とはしない。したがって、DVB `close()` が `DTV_CLEAR` を発行しないことを release blocker または bug と扱わない。

`IFrontend.removeOutputPid(pid)` は、frontend 出力段で PID を除去できる実装が存在しない限り `UNAVAILABLE` とする。soft demux 後段の block list だけで PID を捨てる実装は、frontend-level output PID removal を実装したことにしない。

DVR playback は claim 対象とする。DVR playback の水位通知は AIDL `PlaybackSettings.lowThreshold` / `highThreshold` の説明に合わせ、playback input FMQ の unused space size in bytes を基準に判定する。`SPACE_EMPTY`、`SPACE_ALMOST_EMPTY`、`SPACE_ALMOST_FULL` は threshold 到達時だけ通知し、中間水位では新規status通知を行わない。used bytes を threshold として直接比較してはならない。標準閾値は buffer 容量比で low 25%、high 75% とし、VTS検査用プロファイル では XML 生成時に明示値へ展開する。


## error mapping / scan lifecycle / section overflow / DVR close の契約

`IDescrambler.addPid()` / `removePid()` では、descrambler closed、demux 未設定、key token 未設定、demux generation 消失、再検査時の demux / generation / key state 不整合、closed / runtime-failed source filter を `INVALID_STATE` とする。PID 値不正、foreign / dangling / 別 demux source filter、source filter identity mismatch は `INVALID_ARGUMENT` とする。未対応 `DemuxPid` variant や product capability 未完成は `UNAVAILABLE` に限定する。呼び出し順序や object lifecycle の不整合を `UNAVAILABLE` に写像しない。

Android 14 の Tuner HAL AIDL Rust backend では `IDescrambler.addPid()` / `removePid()` の `optionalSourceFilter` が Rust generated trait 上 non-null `Strong<dyn IFilter>` として現れるため、AOSP Java/JNI/VTS に存在する null filter / PID-only 経路は Android 14 Rust backend で直接受け取れる Rust-only 実装対象に含めない。この構造課題は `IFilter.setDataSource(null)` と同根の Android 14 AIDL/Rust nullable filter 境界として、`future_work/not_planned/android14_aidl_rust_descrambler_pid_only_boundary_report.md` の1ファイル内で管理する。現行実装では non-null source filter 経路の state/error mapping と、同一 descrambler 内の同一PID置換、および別 descrambler 間の同一 demux/generation/PID 排他の Result 契約を対象とする。

frontend scan lifecycle では、`scan_session` は active `Running` scan だけを表す。`Completed` / `Cancelled` / `FailedBackend` / `FailedCallback` / `FailedPanic` は terminal diagnostic として `scan_last_terminal` / `scan_terminal_debug` に保存し、保存後は `scan_session` を `None` にする。`stopTune()` は `scan_session.is_some()` を active scan 判定として使い続けるため、terminal scan が残存して `stopTune()` を `INVALID_STATE` にしてはならない。

section assembler が 8192 bytes 超 section drop または stale partial discard を検出した場合、該当 section filter の diagnostics counter を増やし、`pending_overflow` を立てる。callback worker は既存 `pending_overflow` 経路を使い、payload が空でも `DemuxFilterStatus::OVERFLOW` を通知する。CRC mismatch と malformed section syntax は filter 条件不成立または section event 不成立として非 delivery を維持し、overflow status へ写像しない。

`DvrHal` の `closed` は外部操作を止める gate であり、cleanup 完了状態ではない。DVR cleanup 完了は `cleanup_complete` で別管理する。`close_internal()` / `close_internal_best_effort()` / `fail_dvr_worker()` は、`closed=true` だけを理由に未完了 cleanup の再試行を止めてはならない。3経路は `ExternalClose` / `BestEffortDrop` / `WorkerFailure` の呼び出し元種別を共通 cleanup helper に渡す。cleanup helper は step runner を介して callback worker stop、runtime unregister、queue stop、demux unregister を実行し、各 step の結果を `Success` / `SafeNoOp` / `Failed` / `Unknown` / `SkippedDueToWorkerFailureContext` に分類する。明示 close では最初に観測した error を返しつつ後続 cleanup を続行する。補助系 API は失敗有無を返せないため、その step は成功扱いにせず `Unknown` として残し、`cleanup_complete=true` の根拠にしない。`WorkerFailure` 経路は callback worker 自身から呼ばれ得るため self-join を行わず、worker handle 回収未完了を `SkippedDueToWorkerFailureContext` として扱い、後続の明示 close または Drop 補助 で再試行可能にする。全 step が `Success` または `SafeNoOp` と確認できた場合だけ `cleanup_complete=true` とする。

## lab profile のサービス対応

代表ゲートは次の service 対応で固定する。

| 系統 | frontend | 周波数 | ONID | TSID | service_id | PMT PID | PCR PID | video PID | audio PID | record PID |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| ISDB-T | `FE_ISDBT_UHF` | 557142857 Hz | 32736 | 32736 | 1024 | 256 | 272 | 272 | 273 | 272 |
| BS | `FE_ISDBS_BS` | 1049480000 Hz | 4 | 16400 | 101 | 256 | 272 | 272 | 273 | 272 |
| CS110 | `FE_ISDBS_CS110` | 1613000000 Hz | 6 | N/A | 301 | 256 | 272 | 272 | 273 | 272 |

固定 PID は lab profile の代表値であり、実機検証時は同じ service 対応表に合わせる。製品 scan では PMT から得た PID を使う。

## BS と CS110 の選局契約

BS は IF 周波数と stream selector を併用する。HAL外部契約では、earth_pt1/DVB backend と px4 backend のいずれも TIS の BS TSID 表から渡された TSID を受け付ける。px4 backend に限り、周波数帯と相対TS番号の併用も受け付ける。px4 backend は BS `STREAM_ID` の TSID 値をそのまま legacy `slot` へ渡し、BS `RELATIVE_STREAM_NUMBER` の相対TS番号値をそのまま legacy `slot` へ渡す。ただし、BS `STREAM_ID` の 0..11 は px4_drv で相対TS番号として解釈されるため受け付けない。earth_pt1/DVB backend は相対TS番号を受け付けず、BS `STREAM_ID` の 0..11 も absolute TSID ではなく相対TS番号レンジとして拒否する。CS110 は周波数のみで選局し、profile、VTS設定、scan message、backend 変換のいずれでも streamId/relative stream number を claim しない。


## scan / tune の責務分担

この節は Tuner HAL から見た責務分担を説明するものであり、日本向け scan 候補表のSSOTではない。選局対象範囲と除外条件の設計契約は tv 直下の `開発規則.md`、候補表の具体値と実行時候補生成は TIS の実装データを正とする。

Tuner HAL は、TIS が生成した explicit tune candidate を検証・変換・実行するだけであり、日本向け候補表、BS TSID 表、CATV周波数表、service candidate table を独自に生成せず保持しない。

日本向け周波数表、CATV周波数表、BS/CS110のTSID表、channel key、サービス検出 の実装データ保持者は TIS とする。選局対象、周波数帯、BS/CS110 selector 境界、CATV 候補範囲の設計契約は tv 直下の開発規則.mdを正とする。Tuner HAL は HAL-generated Japanese scan plan を持たず、TIS が作った explicit candidate を `Tuner.tune()` で受ける。HAL の `scan()` は AOSP/VTS互換の最小実装に限定し、製品の通常 channel scan は TIS の周波数表 + `tune()` ループに寄せる。

TIS が持つ候補範囲は、地上波UHF、CATV、BS、CS110を含める。地上波UHFとCATVは周波数候補をそのまま試す。CS110は周波数帯だけで試し、frontend stream id / relative stream number を要求しない。BSだけは同一周波数に複数TSが存在するため、TIS が持つBS TSID表に含まれる同一IF周波数上のTSID候補をすべて試す。px4 backend は BS `STREAM_ID` の TSID 値と BS `RELATIVE_STREAM_NUMBER` の相対TS番号値をそのまま legacy `slot` へ渡し、BS `STREAM_ID` の 0..11 は拒否する。earth_pt1/DVB backend はTSIDをそのまま `DTV_STREAM_ID` に渡すが、BS `STREAM_ID` の 0..11 は absolute TSID ではないため拒否する。

実行時候補生成では、TIS が持つ BS TSID 表だけを正とする。px4 backend 側に TSID から legacy slot への変換表を持たない。TIS から渡された absolute TSID はそのまま px4 legacy API の `slot` へ渡し、px4 専用の相対TS番号もそのまま `slot` へ渡す。absolute TSID として 0..11 が渡された場合は、全backendで相対TS番号レンジとして拒否する。TSID 直渡しにより、TIS 候補表と px4 側 TSID 表の一致確認は r51 修正完了条件から削除する。

この px4 BS `STREAM_ID` direct-slot 契約は、対象 kernel driver が本プロジェクトで採用する px4_drv `feat/android-ddk` 系、すなわち BS legacy `slot >= 8` reject が無効化され、`slot` 値を absolute TSID として `set_stream_id()` へ渡せる実装であることを前提にする。公開 `nns779/px4_drv` develop 相当のように BS `slot >= 8` reject が有効な driver では、absolute TSID direct-slot 経路は使用不可であり、その product で px4 BS `STREAM_ID` 対応を claim してはならない。HAL は互換 fallback として TSID→relative slot 変換表を復活させない。driver 前提が満たせない場合は、TIS/profile/VTS 設定側で px4 BS absolute TSID 経路を使わない構成にする。

CATV も TIS の製品 scan 候補表に実装データとして追加する。CATV候補表は C13〜C63 に固定する。MID band は C13〜C22、SHB band は C23〜C63 とし、中心周波数は ARIB STD-B21 Appendix 10 の `+1/7 MHz` オフセット込みで保持する。C22 は `167 + 1/7 MHz`、C23 は `225 + 1/7 MHz` であり、C21からC22、C22からC23は単純な6MHz連続として計算しない。地上UHF候補表とCATV候補表はどちらもTIS側が正であり、Tuner HAL はCATV scan planを自前生成しない。TIS はCATV候補を explicit tune candidate としてHALへ渡し、px4 backend は渡されたCATV frequencyをlegacy `freq_no/addfreq` へ変換するだけにする。

この節に現れる UHF、CATV、BS、CS110 の範囲説明は、Tuner HAL の独立した候補表定義ではない。値の更新が必要になった場合は、まず `開発規則.md` の設計契約と TIS の候補表実装を更新し、Tuner HAL 側は explicit tune request の validation と backend adapter だけを追従させる。

VHF 1〜12ch は開発規則.mdで恒久的にスコープ外であり、Tuner HAL はVHF候補表、VHF向けpx4変換、VHF lab profileを持たない。

CATVをスコープに含めるため、TIS の製品 scan table は地上UHFだけを前提にしてはならず、CATV C13〜C63 も候補として保持する。

Tuner HAL 側に置いてよい周波数・サービス関連データは、次に限定する。

- VTS / lab profile 用の代表点
- TIS から渡された explicit tune request を backend ioctl へ落とすための backend adapter
- px4 legacy API 用の `freq_no / slot / addfreq` 変換
- explicit tune request の validation に必要な最小境界値

これらは product scan candidate table、サービス検出 SSOT、channel display number、BS/CS110 TSID table、TvProvider metadata の SSOT ではない。製品 scan 候補表、BS/CS110 TSID 表、CATV 中心周波数表、display number、channel key、TvProvider 登録用 metadata は TIS 側を正とする。

VTS / lab profile は代表点だけでよく、全 CATV 候補の実波存在を VTS pass 条件にはしない。

`Tuner.scan(AUTO_SCAN)` を実装する場合も、HALが日本向け候補列を生成しない。TISが明示した1候補に対する一回限りのscanとして扱い、継続探索はTISが次のcandidateを投入する。


## section filter / EIT schedule 上限

`numBytesInSectionFilter` は section payload の最大長ではなく、section filter condition の byte幅として扱う。mask / filter byte 幅は16 bytesを維持する。

`bitWidthOfLengthField` は r51 TS-only profile では `0` と `12` だけを受理し、内部的に `12` へ正規化する。その他の値は `INVALID_ARGUMENT` として configure 時点で拒否する。section assembly、CRC、section condition 判定は同じ正規化済み length field width を使い、condition 判定だけが隠れ 12bit 固定になる実装を残してはならない。


EIT schedule を扱うため、section assembler と section filter delivery が受け入れる assembled section payload の製品上限は8192 bytesに固定する。8192 bytesは filter condition 幅とは別定数にし、PMT/CAT/SDT/NIT/EIT/ECM の section delivery、FMQ書き込み、`SectionEvent.dataLength`、buffer overflow判定で一貫して使う。8192 bytesを超えるsectionは破損または対象外としてdropし、診断counterへ記録する。

PUSI到達時の `pointer_field` は、直前の未完了sectionに対して pointer bytes の範囲だけを合法なtailとして扱う。pointer bytesで直前sectionが完了しない場合、または `pointer_field == 0` で未完了sectionが残っている場合は、旧partial sectionを新section本文へ連結してはならない。旧partial sectionは破棄し、stale partial discard 診断counterへ記録してから `1 + pointer_field` の位置を新section開始として扱う。

## queue overflow / drop 通知方針

internal queue overflow を first-class event として扱う。soft demux 内部 queue、filter delivery queue、DVR record output queue、AV shared buffer、FMQ write のいずれで payload drop または write failure が起きても、無通知破棄 にしてはならない。queue push API は成功、旧データ破棄、新データ破棄、full/backpressure、closed を区別できる結果型を返し、破棄バイト数 / drop packets を診断 counter に必ず反映する。

filter runtime state と DVR runtime state は pending overflow を持つ。callback worker は FMQ write failure だけでなく internal queue drop も overflow 通知対象にし、次回 callback 周期で `OVERFLOW` / overflow status を必ず上位へ通知する。section / PES / record / DVR raw TS で payload が欠落した場合、上位から欠落を観測できない正常短縮として扱ってはならない。

用途別 drop policy は次で固定する。

| path | 方針 |
|---|---|
| live AV | 低遅延優先。古い AV payload の 旧データ破棄 を許容する。ただし overflow event と drop counter は必須。 |
| section | overflow event と drop counter を必須にする。EIT / PMT / CAT 等の欠落を上位が検知可能にする。 |
| PES | overflow event と診断 counter を必須にする。 |
| record / DVR raw TS | 大容量化して極力 drop を避ける。drop した場合は record status / diagnostics に必ず出す。 |

queue 容量は profile 依存にできる構造にする。VTS/lab profile の小容量で overflow test を行えることと、product profile で record / DVR raw TS を大容量化できることの両方を満たす。overflow 時に古いデータを捨てるか新しいデータを捨てるかは用途別に固定し、live AV の 旧データ破棄 方針を section / record path に流用してはならない。


`QueuePushOutcome` は 受理バイト数、破棄バイト数、破棄要素数、旧データ破棄/新データ破棄、overflow を区別する。filter queue で overflow した場合は runtime state の `pending_overflow` を立て、callback worker が payload 有無にかかわらず次周期で `DemuxFilterStatus::OVERFLOW` を通知する。record DVR output queue は 1サービスTS録画 用に 新データ破棄 方針を採り、full 時に新規 TS packet を 無通知破棄 せず `RecordStatus::OVERFLOW` へ伝播する。

## フィルタ状態破棄境界と遅延通知方針

filter の `stop()`、`flush()`、`configure()`、上流フィルタ登録解除 は 状態破棄境界 として扱う。`stop()` / `flush()` は queue、queued bytes、pending overflow、pending start event、delay runtime、filter-local section/PES runtime を破棄し、stopped filter から ペイロード排出 / `DATA_READY` / データイベント を出さない。`configure()` は既存 condition、既存 PID、既存 AV stream type binding、既存 上流接続 を無効化し、`data_source_filter_id` を必ず clear する。下流フィルタ が必要な場合は 再設定 後に `setDataSource()` で明示的に再接続する。

上流フィルタ登録解除 時は、その filter を `data_source_filter_id` として保持している 下流フィルタ を stopped / unlinked 状態にし、下流の queue、queued bytes、pending overflow、pending start event、delay runtime、filter-local assembler / flush generation を clear する。これにより、既存 upstream 由来の payload が後続 start / re-link 後に配送されないことを保証する。

`FilterDelayHint::時間遅延指定` は queue-empty → non-empty の各 まとまり ごとに再armする。start/configure直後の1回限りdelayではない。payload queue が空の filter に新規 payload が入った時点で 期限 を再設定し、最初の まとまり delivery 後に queue が空になった場合、次 まとまり は再び time delay を受ける。

## CAS と descrambler の境界

CAS HAL 本体はプレースホルダーのままにする。`IDescrambler` は AOSP Tuner HAL 面として実装するが、実 CAS token 連携と実波スクランブル解除成功は後続の確認項目とする。

復号鍵台帳には、Rust unit test 専用の deterministic token と、将来 CAS bridge が接続された場合の token を別 origin として登録する。product 経路では CAS bridge 未接続を fail-closed とし、未登録 token、不正 token、空 token、失効 key slot を復号成功として扱わない。Rust unit test 専用 token 登録 API は `#[cfg(test)]` に閉じ、VTS helper や production binary から到達できる設計にしない。

## descramble 失敗時 packet policy

対象 PID の descramble に失敗した場合でも、DVR / raw TS recording path では scrambled TS packet を後段へ pass-through してよい。これは録画済み TS を後からデスクランブルできるようにするための意図的な設計である。

ただし pass-through は clear 成功ではない。packet path は少なくとも次を区別する。

- clear packet
- descrambled packet
- scrambled pass-through packet
- descramble failed packet

Live/AV path、diagnostic、recording metadata、VTS 判定では、scrambled pass-through を `notifyVideoAvailable()` や clear success と混同しない。診断 counter は `NO_KEY`、`BAD_TOKEN`、`CAS_BRIDGE_UNCONNECTED`、`INVALID_TSC`、`MULTI2_FAIL`、`SCRAMBLED_PASSTHROUGH`、`SCRAMBLED_WITHOUT_DESCRAMBLER` を分離し、debug dump 文字列で demux/PID ごとに観測できるようにする。

## px4_drv lock 方針

px4_drv は userspace から RF/carrier lock や demod lock を個別取得できる API を持たない。開発規則.md の既存方針どおり、px4 backend の `DEMOD_LOCK` は `PTX_SET_SYSTEM_MODE`、`PTX_SET_CHANNEL`、`PTX_START_STREAMING` の tune ioctl 系がすべて成功したことだけを 真値 とする。TS packet 到着、PAT/PMT 到着、AV 到着は px4 frontend の `DEMOD_LOCK` 条件に含めない。

この方針は px4 の frontend status だけの設計であり、視聴可能状態の判定ではない。TIS は `notifyVideoAvailable()` を出す前に、section 到達、PMT/ES PID 解決、AV filter data、decoder/surface の成立を別途確認する。px4 backend は `RF_LOCK` を advertise しない。

## px4_drv chardev open / live TS reader 方針

px4_drv の legacy chardev は同一 device node の二重 open を許さないため、px4 backend は control 用 fd と live TS reader 用 fd を別々に `open()` してはならない。`/dev/px4video*` family は `PTX_SET_SYSTEM_MODE`、`PTX_SET_CHANNEL`、`PTX_START_STREAMING`、TS read を同一 open instance から扱う前提にする。

px4 backend は control fd を一度だけ open し、live TS reader はその `File` を `try_clone()` / fd duplicate 相当で複製して使う。TS pump は nonblocking fd と `poll()` の組み合わせで動かし、reader 作成のために同じ chardev path を再 open しない。これにより、px4_drv の single-open 制約下でも tune 後に live TS、section、AV、record/DVR path へ packet を流せることを保証する。

tune / scan lock timeout は、backend 種別、ISDB-T、BS、CS110 を問わず一律 5 秒に固定する。timeout は非同期 worker 側で扱い、binder method を5秒間占有しない。

## DVR 方針

Tuner HAL は `IDvr` を claim 対象とする。DVR は 188-byte MPEG-TS のみを受け入れ、192-byte / 204-byte TS、MMT、TLV は扱わない。DVR record gate は ISDB-T、BS、CS110 のすべてに掛ける。TIS の予約 UI と予約スケジューラは 後続対象だが、HAL の `IDvr` record / playback 面は完成状態に固定する。

表明する録画範囲は**1サービスTS録画** とする。サービスPID集合 の SSOT は TIS に置く。TIS は PMT と サービス検出 結果から、PAT、PMT、PCR、video、audio、caption、data、必要な CA 関連 PID を record filter として 接続する。Tuner HAL は service_id を理解して record 対象を自動生成しない。HAL は attach された複数 record filter の 188-byte TS packet を、受信 TS順序 に近い順序を保って record DVR へ multiplex する。

record filter capacity は32を標準値とする。8 PID 前提の VTS/lab PID-record だけに最適化してはならない。PMT 変更時の PID attach/detach は TIS が行い、HAL は started 中の合法的な attach/detach、重複 attach、detach 後 packet delivery 停止、overflow 通知を state machine として扱う。full transport recording mode は claim 対象外とし、将来の診断または full TS dump feature として扱う。

record DVR / raw TS filter path は受信した 188-byte TS packet を製品の録画品質方針として保持する。TEI が立った packet、duplicate continuity counter の packet、scrambled pass-through packet は、録画・診断・後段デスクランブルのために record path へ到達させる。一方で、section / PES / AV assembly は破損 packet や duplicate packet による二重組み立てを避けるため、TEI packet と duplicate continuity packet を assembly 入力から除外する。これは AOSP が TEI / duplicate の drop/keep policy を明示しているためではなく、日本向け製品の録画品質と parser 安定性を両立するための固定設計である。

DVR playback は claim 対象とする。playback は client から HAL へ TS を入れる入力方向であり、playback injection payload を record/output DVR queue に積んではならない。`inject_playback_payload()` は playback 専用 stats を更新し、playback 起源の TS として demux/filter 入力へ渡すだけにする。frontend/live 起源 TS と playback 起源 TS は routing origin を分離し、playback 起源 TS では direct record filter delivery でも 下流フィルタ propagation でも record DVR mirror を行わない。record/output queue への mirror、record DVR stats の更新、record callback の wake は行わない。

playback 専用 stats は少なくとも injected bytes、injected packets、malformed packets、dropped bytes を持つ。malformed TS は drop + diagnostic を標準方針とし、1 packet の malformed input で playback stream 全体を fail させない。playback input FMQ の `PlaybackStatus` は start 直後・周期 callback ともに playback input FMQ の実 fill / unused write space を唯一の水位 source とし、record/output queue の `queued_bytes` を流用しない。playback consumer worker は `ManagedWorker` / `WorkerSignal` に接続し、close / Drop / fail-closed で stop request → wake → join の順に停止する。

playback input FMQ の stream boundary 方針は次のとおり固定する。start 前に client が prefill した bytes は保持し、start 後に playback TS として読む。started=false 中は worker が FMQ を読まない。stop / flush 時は playback input FMQ と packet assembler residual を drain/discard し、dropped bytes diagnostic counter と log に記録する。stop / flush 後に client が新たに書いた bytes は started=false 中には読まず、直前の stop / flush で既存 stream 境界が drain 済みであることを前提に、次 start の prefill として扱う。playback flush は playback input FMQ、packet assembler、playback stats だけを reset し、record/output queue を破壊しない。record DVR flush は record output queue と record stats だけを reset し、playback input queue と playback stats を破壊しない。

## Frontend capability / status 方針

ISDB-T / ISDB-S の frontend capability bitmask は Android 14 AIDL enum 名に基づく固定値とする。ISDB-T は `AUTO | MODE_3`、`AUTO | BANDWIDTH_6MHZ`、`AUTO | MOD_DQPSK | MOD_QPSK | MOD_16QAM | MOD_64QAM`、`AUTO | CODERATE_1_2 | CODERATE_2_3 | CODERATE_3_4 | CODERATE_5_6 | CODERATE_7_8`、`AUTO | INTERVAL_1_32 | INTERVAL_1_16 | INTERVAL_1_8 | INTERVAL_1_4`、`AUTO | INTERLEAVE_3_0 | INTERLEAVE_3_1 | INTERLEAVE_3_2 | INTERLEAVE_3_4` を advertise する。ISDB-S は `AUTO | MOD_BPSK | MOD_QPSK | MOD_TC8PSK` と `AUTO | CODERATE_1_2 | CODERATE_2_3 | CODERATE_3_4 | CODERATE_5_6 | CODERATE_7_8` を advertise する。

`RF_LOCK` は backend が RF/carrier acquisition を別途取得できる場合だけ advertise する。DVB / earth_pt1 backend は Linux DVB `FE_READ_STATUS` が返す `FE_HAS_CARRIER` を `RF_LOCK`、`FE_HAS_LOCK` を `DEMOD_LOCK` に対応させる。px4_drv backend は RF/carrier lock を返す API を持たないため、px4 の擬似 lock は `DEMOD_LOCK` のみに使い、`RF_LOCK` には使わない。

`SNR` と `SIGNAL_STRENGTH` は、r51 では `statusCaps` に含めない。DVB / earth_pt1 の `FE_READ_SNR` と `FE_READ_SIGNAL_STRENGTH`、px4 の `PTX_GET_CNR` は target driver / device 状態によって read 時に失敗し得る optional telemetry であり、起動時列挙時点で frontend entry の固定 capability として証明できないためである。これらの optional telemetry は diagnostic 内部値として保持してよいが、AOSP statusCaps 上の supported status として advertise してはならない。

`SIGNAL_QUALITY` は、backend ごとに根拠ある合成値を返せる場合だけ `statusCaps` に含める。DVB / earth_pt1 backend の `SIGNAL_QUALITY` は Linux DVB `FE_READ_STATUS` status bit の lock 進捗を 0〜100 に正規化した値とする。px4 backend は `PTX_GET_CNR` を安定取得できることを frontend entry の capability として固定できない限り、`SNR` と `SIGNAL_QUALITY` を advertise しない。いずれも `DEMOD_LOCK` や `RF_LOCK` の代替ではなく、UI/diagnostic 用の合成指標である。未取得 telemetry を `SIGNAL_QUALITY=0` として成功返却してはならない。


## live AV filter / FMQ 方針

live AV filter を正式スコープに含める。AV filter は non-passthrough の `MediaEvent` 経路を実装し、`MediaEvent` から framework が取得できる shared handle / linear block 相当の実体を返す。FMQ / EventFlag もスコープに含め、section、PES、record、DVR では custom ring ではなく official FMQ shim を使う。AV payload は FMQ / EventFlag へ載せず、shared memory + MediaEvent に一本化する。

r51 では AV passthrough を claim しない。`DemuxFilterAvSettings.isPassthrough=true` は configure 時点で `UNAVAILABLE` とし、成功 no-op または無配送の AV filter として受け入れてはならない。r51 が claim する AV 経路は non-passthrough `MediaEvent` + shared memory 経路だけである。

ここで「FMQ / EventFlag もスコープに含める」とは、section、PES、record、DVR の official FMQ 接続を完了条件に含めることと、live AV filter の正式 delivery を完了条件に含めることの両方を意味する。前者は custom ring を残さず official FMQ/EventFlag に接続する責務であり、後者は `MediaEvent` + shared handle によって framework が live AV payload を正式に受け取れるようにする責務である。

AV filter の正式経路は `MediaEvent` + shared handle とし、FMQ だけにESを流す経路を live AV filter の完成条件にしない。AV payload は通常 queue / AV補助queue を含む FMQ / EventFlag 経路に載せない。診断は shared memory delivery 結果、`MediaEvent`、callback status、AV shared diagnostics で行う。

`openFilter()` は section / PES / record / PCR / AV の種別にかかわらず AV shared memory を確保しない。`/dev/dma_heap/system` への依存は、AV filter が `getAvSharedHandle()` に到達した時点の lazy allocation に限定する。非 AV filter の `openFilter()`、`configure()`、`start()`、`getQueueDesc()` は dma heap の存在・権限・SELinux 許可に依存してはならない。AV filter の `start()` は AV stream type が確定済みであれば成功可能とし、`getAvSharedHandle()` 未実行だけを理由に失敗させない。ただし shared handle 未 export 中は framework/JNI が消費できない成功風 `MediaEvent` を出さず、drop/overflow 診断へ落とす。binder 公開経路と soft_demux core の `start_filter_result()` は、AV stream type 未設定の AV filter start を `InvalidState` として拒否し、再設定 は既存の AV stream type binding を必ず破棄する。

AV shared memory allocator は Rust から `/dev/dma_heap/system` に直接 ioctl せず、既存 FMQ shim と同じ native shim 境界を通じて AOSP `libdmabufheap` の `BufferAllocator::Alloc("system", ...)` を使う。unit test では allocator failure 時に memfd fallback を許容するが、production AV 経路では dma-buf allocation failure を AV filter error として返す。確保済み dma-buf への payload 書き込みは fd への `write` / `pwrite` ではなく、`mmap(MAP_SHARED)` した CPU mapping へのコピーで行う。

Android 14系 framework/JNI が受理する `MediaEvent` + shared handle は、Codec2 が `MediaCodec.LinearBlock` として import できる ION / dma-buf 系共有メモリ fd を `NativeHandle` の先頭 fd として持つ形式に固定する。`IFilter.getAvSharedHandle()` は1個の fd を持つ `NativeHandle` と共有メモリ総サイズを返す。shared handle の `NativeHandle.ints` は `[0]` のみとし、先頭 int は Android framework/JNI が memory index として扱える値に固定する。`slot_size` と `slot_count` は HAL 内部の slot 管理状態であり、`NativeHandle.ints` として framework へ公開しない。shared handle方式では、各 `DemuxFilterMediaEvent.avMemory` はempty handleでよいが、`avDataId` は0以外のslot/frame lifetime IDとし、`offset` と `dataLength` は共有メモリ内のAV access unit範囲を示す。`offset + dataLength` は shared memory total size 未満に収める。zero-length AV payload は MediaEvent として出さず、`av_invalid_payload` 診断 counter と `OVERFLOW` status に反映する。filter の再 `configure()` / `flush()` / `close()` では既存 shared handle lifetime を必ず無効化し、再度 `getAvSharedHandle()` を要求する。

AV payload の `DATA_READY` は、shared memory 上に有効な payload を配置できた場合だけ通知する。shared handle 未 export 中は `av_drop_unexported` 診断 counter と `OVERFLOW` status に反映する。shared slot が確保できない場合は active slot を eviction せず、`av_overflow_no_slot` 診断 counter と `OVERFLOW` status に反映する。shared memory 上に有効な payload を配置できない AV payload は、callback status の `DATA_READY` だけでなく、通常 FMQ / EventFlag の `TUNER_EVENT_DATA_READY` も発生させない。`av_shared_handle_exported=true` なのに shared backing が存在しない状態、shared backing の mutex poison、shared handle export/backing 不整合、active slot collision、slot registry inconsistency、mapping failure、counter failure は drop/overflow に偽装せず、internal error variant 名を診断に残して対象 filter worker を fail-closed にする。`releaseAvHandle(handle, avDataId)` はevent側handleがemptyでも `avDataId` だけで該当slot/frameを解放できなければならない。clear-media-path であり、`isSecureMemory=false` に固定する.

AV payload は AV補助queue にも書き込まない。live AV filter の payload delivery は shared AV memory slot と `MediaEvent` に一本化する。shared AV memory slot は active slot を eviction せず、空き slot がない場合は `av_overflow_no_slot` と `OVERFLOW` status に落とす。section、PES、record、DVR raw TS path にはAVのdrop/backpressureを波及させない。

## A/V sync 方針

AV filter を claim する demux は AOSP の `getAvSyncHwId(Filter)` と `getAvSyncTime(int)` の契約に沿って A/V sync ID と 90kHz timestamp を返す。`getAvSyncHwId()` は同一 demux 内の audio/video main filter にだけ deterministic ID を返し、section、PES、record、closed filter には `UNAVAILABLE` を返す。

`getAvSyncHwId()` は、対象 filter が audio/video main filter であり、かつ soft demux が PCR 由来の source clock を既に保持している場合だけ sync ID を返す。AOSP CTS は `INVALID_AV_SYNC_ID` を許容する一方、valid ID を返した場合は `getAvSyncTime(id)` が valid timestamp を返すことを期待するため、PCR 未観測時に valid ID を先出ししない。

`getAvSyncTime()` は sync ID が指す AV filter を検証し、soft demux が最後に観測した PCR base を基準に、観測時点からの経過時間を 90kHz clock に換算して加算した current timestamp を返す。PCR が未観測の場合は PTS を代用せず `UNAVAILABLE` を返す。PTS は presentation timestamp であり、AOSP が要求する current A/V sync clock の代替にしない。PCR の 33-bit wrap は内部で extended 90kHz 値へ伸長して単調性を保つ。

## AV filter start / shared handle / A/V sync 境界

r51 リリース前までに Tuner HAL が満たす必須境界を以下に固定する。

- AV filter の `start()` は、`getAvSharedHandle()` が未実行であることだけを理由に失敗させない。AV stream type 未設定など、filter 自体の状態不整合だけを `INVALID_STATE` とする。
- shared handle がまだ export されていない間は、framework/JNI が消費できない成功風 `MediaEvent` を出さない。
- shared handle 未 export 中に AV payload を受け取った場合は、callback status の `DATA_READY` と FMQ / EventFlag の `TUNER_EVENT_DATA_READY` を出さず、通常 queue / AV補助queue に payload を書き込まず、`av_drop_unexported` と `OVERFLOW` status に残す.
- A/V sync は、PCR が未観測であれば valid sync ID を返さない。valid sync ID を返す場合は、`getAvSyncTime(id)` が valid 90kHz timestamp を返せる状態に限る。
- PTS は current A/V sync clock の fallback として使わない。
- PCR と monotonic clock の対応付けによる最小 wallclock 補間は維持する。
- `AvSyncState` は、PCR PID 明示管理、service clock、jitter smoothing、PLL / clock discipline を後続で接続できる構造にする。

r51 リリース後の後続 future_work として、以下は今回の実装範囲外にする。

- PCR PID 明示管理。
- service clock モデル。
- jitter smoothing。
- PLL / clock discipline。
- 複数 clock source の品質評価。
- より厳密な CTS / VTS / 実波ベース補正。


## LNB 固定 profile

対象ハード構成は px4_drv 系と earth_pt1 系に限定する。px4_drv 系で LNB 電源を成功扱いにするのは、対応デバイス仕様で 15V 出力が確認できる `px4video*` family のみとし、`pxmlt5video*`、`pxmlt8video*`、`isdb6014video*` は安全側に倒して `NONE` のみ成功にする。earth_pt1 系は `NONE`、`11V`、`15V` だけを受け付ける。tone、DiSEqC、satellite position switching は恒久的に未対応であり、`POSITION_UNDEFINED` 以外の satellite position、tone ON、自動 tone、DiSEqC message は `UNAVAILABLE` とする。汎用 DVB profile は作らない。

LNB は satellite frontend の所有物として扱い、shared LNB の余地は置かない。`setLnb(lnb_id)` は当該 satellite frontend に紐付いた LNB ID だけを受け付け、別 frontend の LNB ID、地上波 frontend への LNB attach、不明な LNB ID は失敗させる。

`ILnb.close()` は reset-on-close として扱う。public `close()` は callback を消すだけでは成功扱いにせず、LNB registry の voltage を `NONE`、tone を `NONE`、satellite position を `UNDEFINED` に戻し、generation を進め、当該 LNB を選択中の frontend backend へ reset state を反映してから closed state を確定する。reset 反映に失敗した場合は `close()` を成功扱いにせず、Drop 経路の 補助 cleanup と public Binder close の完了条件を分離する。

## 復号鍵台帳

`IDescrambler.setKeyToken()` が受け取る値は復号鍵そのものではなく、不透明な参照値である。Tuner HAL はこの参照値で復号鍵台帳を引き、内部の `DescramblerKeySlot` に変換する。Binder 境界を越える byte列に MULTI2 の system key、CBC 初期値、偶数鍵、奇数鍵を入れてはならない。

## デスクランブル gate

VTS/lab config には descrambling flow を置かない。VTS 用 XML に ECM filter や `<descramblers>` を生成せず、clear live / DVR / explicit tune の接続確認に限定する。Tuner HAL は PMT/CAT/SDT/ECM/EMM 等の section payload delivery、`IDescrambler`、`setKeyToken()`、`addPid()` / `removePid()`、token lookup 境界、未接続・bad token・expired token 診断までを確認対象とする。CA情報 / service metadata の semantic extraction、ECM/EMM filter 開始方針、MediaCas/CAS bridge 呼び出し、不透明な参照値の取得試行、Tuner descrambler への接続判断、未接続診断の上位制御は TIS / arib_si_engine_rs / fake CAS テストまたは実機診断で確認する。CAS HAL 本体はプレースホルダーのため、実波スクランブル解除成功は後続の確認項目とする。Tuner HAL の packet 単位のデスクランブル中核は、単体テスト内で復号鍵台帳へ既知鍵を登録して確認する。


## IDescrambler optionalSourceFilter 境界

AOSP Tuner SDK / JNI / VTS には `IDescrambler.addPid()` / `removePid()` の source filter を null として扱う PID-only 経路が存在する。一方、開発規則が対象とする Android 14 / LineageOS 21 系の Tuner HAL AIDL Rust backend では、`optionalSourceFilter` が `@nullable` 付きではないため、Rust generated trait 上は non-null `Strong<dyn IFilter>` として現れる。

Android 14 Rust backend 方針では、AOSP stable AIDL を変更しない。vendor 独自 `@nullable` 追加、AOSP frozen AIDL の改変、C++/NDK wrapper 追加、Rust raw Binder transaction parser 追加は行わない。したがって、PID-only / null source filter 経路は Rust-only 実装対象から除外し、`android14_aidl_rust_descrambler_pid_only_boundary_report.md` で構造課題として別管理する。

`IDescrambler.addPid()` / `removePid()` の Rust backend 実装対象は、Android 14 Rust generated trait で受け取れる non-null source filter 経路の state / argument / unavailable mapping に限定する。

`optionalSourceFilter != null` の場合、source filter が自 HAL 内の local `IFilter` であり、同じ demux に属し、closed / runtime-failed ではなく、demux registry 上の open filter record として実在することを検証する。PID 登録は `demux_id`、demux open generation、source filter id、source filter delivery generation を保存する。demux close / reopen、source filter close / unregister、filter 再設定 / flush により世代が変わった登録は descrambler snapshot 生成時に prune し、古い key/PID 登録を新しい demux または新しい source filter に適用しない。

同一 descrambler 内では PID 登録表の主キーを PID とし、同一PIDに対する `addPid(pid, sourceFilter)` は既存登録を新しい source filter generation で置換する。これは AOSP Java API の同一PID置換 semantics に合わせる。別 descrambler 間では、同一 demux / demux generation / PID を二重に復号対象へ登録しないため、既に他の active descrambler が同一PIDを保持している場合は `INVALID_STATE` とする。この別 descrambler 排他は同一 descrambler 内の置換 semantics とは別契約であり、PID値・source filter object 自体の不正ではなく、active descrambler registry 上の所有状態が当該 `addPid()` 操作を許さない状態衝突として扱う。後段 packet path の二重復号と key slot競合を避けるための HAL 内部資源管理である。

error mapping:
- `INVALID_STATE`: descrambler closed、demux 未設定、key token 未設定、source filter closed / runtime-failed、demux generation 消失または再検査時 state 不整合、別 active descrambler による同一 demux / demux generation / PID 所有衝突。
- `INVALID_ARGUMENT`: invalid PID、foreign filter、別 demux filter、not-open / dangling local filter handle。
- `UNAVAILABLE`: unsupported `DemuxPid` variant、または product capability 未完成に限定する。

## DVB backend の対応表

DVB backend は frontend index と同じ demux index / dvr index を使う。`adapterN/frontendM` は `adapterN/demuxM` と `adapterN/dvrM` に対応する。demux が別 frontend の TS を読む構成は advertise しない。source 選択 ioctl が失敗した場合は tune / scan / record を成功扱いにしない。

## 診断可観測性の固定

現行設計では CAS bridge はまだ production 接続しない。`register_from_cas_bridge()` は将来接続用の登録口だが、現時点の非 test product 経路からは呼ばれない。production TIS は placeholder token または診断専用tokenを `setKeyToken()` へ渡してはならない。 `production token` は r52 以降に CAS HAL 本体が発行する復号用の不透明参照値だけを指す。`fake token`、`diagnostic token`、`placeholder token` は production 経路で復号成功に使ってはならない。

`IDescrambler.setKeyToken()` に到達する token は、Tuner SDK API の制約に合わせて、長さ 1〜16 byte の opaque byte array のみにする。ただし Android 14 系の `Tuner.VOID_KEYTOKEN` は 1 byte token `[0x00]` として扱い、current key removal 用の有効 token とする。空 token `[]` は VOID token ではなく、常に `INVALID_ARGUMENT` と内部診断 `BAD_TOKEN` に落とす。16 byte を超える文字列 token を `setKeyToken()` に渡してはならない。

`maleicacid-cas-desc-token-*`、`maleicacid-placeholder-desc-token*`、既存 TIS 側の `maleicacid-kari-token-*` は、設計文書上の診断名またはログ上のラベルであり、Tuner SDK API 経由で渡す実 token ではない。単体テスト、fake CAS、診断注入で同等のケースを表現する場合も、`setKeyToken()` に渡す byte array は 16 byte 以下の fixed test token とし、長い診断名は test case 名、lookup table の説明、diagnostic dump の表示名に限定する。

これらの診断 token origin を受け取った場合は、復号成功ではなく `CAS_BRIDGE_UNCONNECTED`、`BAD_TOKEN`、`EXPIRED_KEY_SLOT` など該当する診断へ落とす。

`IDescrambler.setKeyToken()` は、最初に `[0x00]` を `Tuner.VOID_KEYTOKEN` として処理し、registry lookup に流さず current key slot のみ解除する。PID 登録は維持する。次に空 token `[]` と形式不正 token を registry lookup 前に拒否し、空 token は `INVALID_ARGUMENT` と内部診断 `BAD_TOKEN` に固定する。未登録 token と CAS bridge 未接続 token は通常 token として registry lookup 後に区別して診断する。診断を迂回する token 解決 API は production 経路へ公開しない。

デスクランブル診断は、`dump_descrambler_diagnostics_for_debug()` の dump 文字列と `maleicacid-tuner-hal-descrambler-diagnostic` ログで観測する。dump には demux、PID、`CLEAR_PACKET`、`DESCRAMBLED`、`SCRAMBLED_PASSTHROUGH_FOR_RECORDING`、`MALFORMED_PACKET_FOR_RECORDING`、`DESCRAMBLE_FAILED`、`INVALID_PACKET_SIZE`、`BAD_SYNC_BYTE`、`INVALID_AFC`、`INVALID_ADAPTATION_FIELD`、`INVALID_TSC`、`SCRAMBLED_WITHOUT_PAYLOAD`、`NO_KEY`、`BAD_TOKEN`、`CAS_BRIDGE_UNCONNECTED`、`EXPIRED_KEY_SLOT`、`MULTI2_FAIL`、`SCRAMBLED_WITHOUT_DESCRAMBLER` を含める。`SCRAMBLED_PASSTHROUGH_FOR_RECORDING` は後段デスクランブル可能な録画 TS を残すための pass-through であり、clear 成功を意味しない。malformed / undefined な TS-frame-like packet の録画保存は `MALFORMED_PACKET_FOR_RECORDING` で別管理し、`InvalidPacketSize` / `BadSyncByte` は record-DVR raw TS に保存しない。

`MALEICACID_TUNER_HAL_DESCRAMBLER_DIAGNOSTIC_FILE` を設定した デバッグビルドまたは立ち上げ検証環境では、Tuner HAL service が 5 秒間隔で同じ descrambler diagnostic dump を指定ファイルへ書き出す。Stable AIDL には vendor 独自メソッドを追加しない。


### 失効 token 診断

`maleicacid-expired-desc-token-*` は診断名であり、`setKeyToken()` に渡す実 token ではない。失効 token の単体テストでは、16 byte 以下の fixed test token を key registry に登録し、その registry entry を expired state にすることで `EXPIRED_KEY_SLOT` を発生させる。

`setKeyToken()` は、空 token、16 byte 超 token、未登録 token、失効済み token、CAS bridge 未接続 token を区別して診断 counter に記録する。`[0x00]` は `BAD_TOKEN`、unknown token、CAS bridge 未接続には混ぜず、key 未設定状態でも success no-op とする。空 token `[]` は registry lookup、current key slot 変更、PID 登録変更を行わない。ただし Tuner SDK API 経由の production / integration 経路では、16 byte 超 token は Java層で invalid argument になり得るため、HAL 内部診断へ到達することを前提にしてはならない。

## B25 packet デスクランブル中核の範囲

現行 Tuner HAL は、libaribb25 相当の B25 全体実装であるとは主張しない。Tuner HAL に実装済みなのは、188 byte TS packet の payload に対する MULTI2 復号中核、odd/even key 選択、adaptation field を壊さない payload offset 判定、復号成功時の scrambling_control 正規化、復号失敗時の録画向け scrambled pass-through 診断である。

ECM / EMM 処理、カード I/O、CAS 権利判定、CW 取得、不透明 token 発行、B25 system key / CBC 初期値 / data key を CAS 側から安全に供給する経路は CAS HAL または CAS bridge の責務であり、現行設計では placeholder のままである。そのため、現行ロジックの OK 判定は「Tuner HAL の packet 単位のデスクランブル中核と診断境界が静的に整った」という意味であり、「CAS 通信部だけを除いて libaribb25 の TS→TS B25 処理系が全て完成した」という意味ではない。

## LNB profile 判定表

LNB profile は sysfs `DEVNAME` または `/dev` basename と earth_pt1 の sysfs driver basename で決定する。HAL は以下の表を実装に持つ。

| device node prefix | LNB profile | 成功する voltage |
|---|---|---|
| `px4video*` | `Px4Device15VOnly` | `NONE`, `15V` |
| `pxmlt5video*` | `NoPower` | `NONE` |
| `pxmlt8video*` | `NoPower` | `NONE` |
| `isdb6014video*` | `NoPower` | `NONE` |
| `isdb2056video*` | `NoPower` | `NONE` |
| `pxm1urvideo*` | `NoPower` | `NONE` |
| `pxs1urvideo*` | `NoPower` | `NONE` |
| `isdbt2071video*` | `NoPower` | `NONE` |


`pxmlt5video*` は対応デバイス仕様で LNB 電源非対応のため `15V` を advertise しない。`pxmlt8video*` と `isdb6014video*` は LNB 電源仕様が未確定のため、現行設計では product profile による明示 opt-in を作らず `NoPower` に固定する。未確認デバイスを 15V 成功扱いにする silent overclaim は禁止する。

DVB frontend は sysfs driver basename が `earth-pt1` の場合だけ `EarthPt1FixedLnb` として採用する。frontend name に `tc90522` が含まれるだけでは採用しない。

`EarthPt1FixedLnb` は `NONE`、`11V`、`15V` だけを成功にする。`13V`、`18V`、tone、DiSEqC、satellite position switching は成功扱いしない。

## export ID と VTS profile の固定

Tuner HAL が framework へ export する frontend ID は px4_drv の numeric index だけに依存しない。`px4video0` と `pxmlt5video0` のように異なる device family が同じ unit index を持つ場合でも、HAL の frontend ID と physical group ID は衝突してはならない。device family code と unit index を組み合わせ、1,000,000 番台の px4 frontend ID として export する。px4 frontend の `exclusiveGroupId` は unit index 単独値ではなく、device family code と unit index を含む packed physical group id として返す。

VTS設定 は `profiles/*.yaml` から `tools/render_vts_config.py` で生成する。LabProfile は ISDB-T、BS、CS110 をすべて持ち、ProductProfile や DiagnosticProfile と混ぜない。VTS検査用プロファイル は代表 PID による 188-byte TS 録画/再生経路 接続確認に使うが、設計 claim は 1サービスTS録画 であり、8 PID 前提の 検査専用 実装に縮退させてはならない。TIS 録画 UI や予約スケジューラとは結びつけない。製品向け復号フロー は VTS検査用プロファイル で claim せず、ECM filter と `<descramblers>` は生成しない。

## product 統合手順

`maleicacid.tv.tuner_hal-service` は `Android.bp` の `init_rc` と `vintf_fragments` で init rc と VINTF fragment を install する。製品側の product makefile では `config/product_integration.mk` を継承し、`maleicacid.tv.tuner_hal-service`、`maleicacid_tuner_hal_vts_config_aidl_v2`、`maleicacid_tuner_hal_ueventd_rc` を `PRODUCT_PACKAGES` に追加する。

ueventd rc は `maleicacid_tuner_hal_ueventd_rc` prebuilt package として product に入れる。SELinux policy は product makefile ではなく BoardConfig 系で `config/BoardConfigVendorSePolicy.mk` を include し、`BOARD_VENDOR_SEPOLICY_DIRS += vendor/maleicacid/tv/tuner_hal/sepolicy` を取り込む。

VINTF fragment と init rc は service module の property で install されるため、product manifest や device rc に同じ内容を二重登録しない。

px4 probe prefix を変更する場合は、`frontend_px4/src/lib.rs` の `PX4_PROBE_PREFIXES`、`config/ueventd.tuner_hal.rc`、`sepolicy/file_contexts` を同時に更新し、static check とロジック確認で一致を確認する。


## Tuner HAL runtime 完了条件

Tuner HAL runtime の修正対象を以下の契約として固定する。

- 対象 tuner device が見つからない場合も HAL service は起動する。probe 結果が空の場合、存在しない frontend を registry に登録せず、`getFrontendIds()` と `getFrontendInfo()` で device absent の frontend を advertise しない。service 起動自体は継続し、device missing の縮退理由 を診断に残す。対象 resource への操作要求が来た場合は `UNAVAILABLE` と診断へ fail-closed する。
- filter ID は HAL 外部へ返す値を demux-local ID のまま維持する。DVR attach/detach、filter data source、AV sync ID 取得では、渡された filter object の内部 owner demux を検証し、owner demux が一致しない filter を `INVALID_ARGUMENT` で拒否する。
- worker は handle 保存先の mutex を確保してから spawn する。保存先を確保できない場合は spawn しない。worker panic は `join_worker_with_diagnostics()` で診断へ残し、detached worker を作らない。
- 長寿命 worker の待機は `Mutex` + `Condvar` を基本とし、stop request → wake → join の順で停止する。`AtomicBool` は close済み / stop要求 / export済みなどの単純 flag に限定し、複合状態同期の代替にしない。`loom` は test-only 候補であり、通常 unit test と静的ロジック確認の代替にはしない。

- r51 で管理対象となる長寿命 worker は、`ManagedWorker` が `JoinHandle` と `WorkerSignal` を所有し、`WorkerSignal` の `Mutex<WorkerSignalState> + Condvar` で stop/work generation を wake する。`WorkerExit` は `Normal` / `StopRequested` / `RuntimeFailure` / `PanicOrJoinFailure` を正式名とする。
- `frontend_tune_worker` / `frontend_scan_worker` の停止は、`AtomicBool + thread::sleep()` polling ではなく、`WorkerSignal::request_stop()` → Condvar wake → `ManagedWorker::stop_and_join()` の順に行う。
- Demux close / live pump failure / worker spawn failure は子 Filter / DVR / runtime I/O を fail-closed にし、close後の既存 child object の `configure()` / `start()` / `getQueueDesc()` などを成功扱いしない。
- frontend source transition は transactional に扱い、new bind / old unbind / record更新 / stream boundary reset の途中失敗時には新 binding をrollbackし、rollback不能なら demux を fail-closed にする。
- public close は critical cleanup の失敗を成功扱いしない。Drop 経路だけ補助 cleanup とし、public Binder close は cleanup 完了後に closed state を確定する。
- DVR start は status interval 分だけ Binder thread を sleep しない。status interval は callback worker の周期だけに使う。
- playback consumer は no data と fatal error を分離する。FMQ read error、demux mutex poison、fatal demux error は worker fatal stop として diagnostics と object state に反映し、後続操作を成功扱いしない。
- px4 close は control FD だけでなく TS reader FD と reader state も解放する。
- px4 の CNR 取得は optional telemetry であり、`PTX_GET_CNR` 失敗だけで lock/status query を fatal error にしない。
- section filter は condition の必要 byte 幅が payload 長を超える場合に match しない。prefix だけ一致した短い payload を match としない。
- `TableInfo.version` は `-1` または `0..31` だけを受け付ける。`-1` は wildcard、範囲外は `INVALID_ARGUMENT` とする。
- PES `streamId` は `0..=255` を明示 `stream_id` として照合し、`-1` だけを wildcard として扱う。その他の負値と `256` 以上は `INVALID_ARGUMENT` とする。`streamId=0` は wildcard ではなく、8-bit 値 `0x00` の明示照合である。
- 入力値不正は `INVALID_ARGUMENT`、未対応 capability は `UNAVAILABLE`、object state 不整合は `INVALID_STATE`、mutex poison や内部整合性崩壊は `UNKNOWN_ERROR` / `HalError::Internal` に写像する。
- CHANGELOG と log message を除き、source comment は日本語に統一する。
- AV filter の `start()` は `getAvSharedHandle()` 未実行だけを理由に失敗しない。shared handle 未 export 中の AV payload は `MediaEvent`、callback status の `DATA_READY`、FMQ / EventFlag の `TUNER_EVENT_DATA_READY` を出さず、通常 queue / AV補助queue に payload を書き込まず、`av_drop_unexported` 診断 counter と `OVERFLOW` status に反映する。slot allocation 失敗時は active slot を eviction せず、`av_overflow_no_slot` 診断 counter と `OVERFLOW` status に反映する。payload サイズ不正 / shared memory 範囲外では `av_invalid_payload` 診断 counter と `OVERFLOW` status に反映する。shared backing mutex poison、shared handle export/backing 不整合、active slot collision、slot registry inconsistency、mapping failure、counter failure は drop/overflow に偽装せず、internal error variant 名を診断に残して filter worker を fail-closed にする.
- A/V sync は PCR 未観測時に valid sync ID を返さず、PTS fallback を持たない。PCR + monotonic 補間を維持し、`AvSyncState` には PCR PID、service clock、jitter smoothing、PLL の後続接続用 field を保持する。

## Tuner HAL の no-panic / degraded boot / fail-closed 境界

Tuner HAL の release runtime path は、public Binder method、worker thread、callback delivery、frontend backend、demux/filter/DVR/descrambler/LNB runtime state の全てで no-panic boundary とする。`unwrap()`、`expect()`、`panic!()`、`unreachable!()`、`todo!()`、`unimplemented!()`、`assert*()`、`dbg!()` を runtime invariant の表現として使わない。HAL service 登録失敗は、panic ではなく明示 log と process exit で fail-fast する。

Target tuner device が存在しない、または権限・device node・driver probing に失敗する場合は degraded boot とする。HAL service 自体は登録するが、存在しない frontend / demux / backend resource を capability として advertise しない。`getFrontendIds()` は実在 probe できた frontend だけを返す。存在しない resource への `openFrontend*`、`tune`、`scan` などの public Binder method は `UNAVAILABLE` または対応する service-specific error を返し、service 起動を panic で中断しない。

Mutex poison は recover-with-inner ではなく fail-closed とする。runtime object の mutex lock に失敗した場合は、対象 object を操作成功扱いにせず、Binder method では `UNKNOWN_ERROR` / service-specific error、内部 HAL path では `HalError::Internal`、非同期 worker では診断 log と停止扱いへ写像する。poison 後に破損可能な状態を継続利用しない。

Public Binder method の error mapping は、入力不正を `INVALID_ARGUMENT`、存在しない object を `NAME_NOT_FOUND`、未対応機能を `UNAVAILABLE`、poison や内部整合性崩壊を `UNKNOWN_ERROR` または `HalError::Internal` 起点の service-specific error に固定する。成功を返す場合は、対象 state mutation または query が poison なしに完了していなければならない。

Worker thread は `spawn_worker()` を通して生成し、entrypoint panic を `catch_unwind` で捕捉して diagnostic log に残す。worker の停止待ちは `join_worker_with_diagnostics()` に集約し、panic stop を黙殺しない。worker 内の mutex poison や backend error は、対象 loop を fail-closed stop または NO_SIGNAL / unavailable 診断へ写像し、panic で HAL process を落とさない。

release HAL path の静的確認では、non-test runtime から `unwrap()` / `expect()` / `panic!()` / `unreachable!()` / `thread::spawn` 直接呼び出し / silent `join()` を禁止する。`#[cfg(test)]` と `tests/` 配下は対象外とする。`spawn_worker()` と `join_worker_with_diagnostics()` は、worker policy を実装する唯一の runtime wrapper として許可する。


## Tuner HAL 固定修正境界

- CS110 は周波数のみで選局する。ISDB-S settings で `streamIdType=UNDEFINED` かつ `streamId=0` の明示未指定、または AOSP SDK の default 表現である `streamIdType=STREAM_ID` かつ `streamId=-1` だけを selector なしとして扱う。CS110 tune request に TSID / relative stream-number selector が指定された場合は `INVALID_ARGUMENT` とする。`streamIdType=RELATIVE_STREAM_NUMBER` の負値、`streamIdType=UNDEFINED` の負値、その他の負値 selector は未指定へ丸めない。
- BS は TSID 指定を要求する。px4 backend だけ relative stream number を受け付け、DVB backend では relative stream number を `INVALID_ARGUMENT` とする。BS `STREAM_ID` の 0..11 は全backendで `INVALID_ARGUMENT` とする。
- Filter / DVR callback failure は silent success にしない。対象 callback registration を cleanup し、対象 object を failed / closed state へ遷移させ、object ID、callback API、binder status を diagnostics に残す。
- Filter / DVR worker は lock failure、registry inconsistency、record 不在、callback failure で silent stop しない。abnormal stop として diagnostics に残し、対象 object を failed / closed state へ遷移させる。
- DVR status interval は callback worker の周期にだけ使う。worker の wait は stop signal で wake 可能な cancellable wait とし、close / Drop / shutdown は interval 満了を待たない。
- `getAvSharedHandle()` は configured AUDIO / VIDEO filter 専用である。非 AV filter と `configureAvStreamType()` 未実行の AV filter は `INVALID_STATE` とし、shared backing を生成しない。
- device missing / open failure は `UNAVAILABLE`、device が存在する状態での runtime ioctl / read failure は `UNKNOWN_ERROR` とする。client invalid input と runtime I/O failure を同じ error path に入れない。
- r51 では filter monitor event を claim しない。`configureMonitorEvent(0)` のみ成功し、非 0 mask は `UNAVAILABLE` とする。通常の `DATA_READY` / `OVERFLOW` / `onFilterEvent()` delivery は monitor mask で抑止しない。
- soft demux の section / PES assembler は、started filter が存在する対象 PID にだけ作成する。filter stop / unregister 後に対象 PID の started filter が残らない場合、該当 PID の assembler state を破棄する。
- `setMaxNumberOfFrontends()` は `0 <= max_number <= default_max` だけを成功させる。負値と `default_max` 超過はどちらも `INVALID_ARGUMENT` とする。
- product runtime の frontend registry は実在 probe できた backend entry だけで構成する。probe 失敗は diagnostics record に残し、degraded frontend entry / test degraded helper / diagnostic degraded helper は作らない。


## frontend settings validation の固定方針

Tuner HAL が advertise する frontend capability は、Android 14 AIDL enum 名に基づく固定 bitmask とする。capability 値の詳細は本書の「Frontend capability / status 方針」を正とし、本節では重複定義しない。

public `FrontendSettings` validation は、advertised capability と矛盾してはならない。`AUTO` だけを受け付け、advertise 済みの具体 enum 値を拒否する実装は禁止する。

explicit range scan は ISDB-T / ISDB-S 共通で claim しない。`endFrequency` が `frequency` と異なる場合は、共通 validation で `UNAVAILABLE` とする。

### ISDB-T validation

- `bandwidth` は `AUTO` または `BANDWIDTH_6MHZ` だけを受け付ける。
- `mode` は `AUTO` または `MODE_3` だけを受け付ける。
- `modulation` / `coderate` / `guardInterval` / `timeInterleave` は、advertised capability に含まれる値だけを受け付ける。
- `timeInterleave` は mode 3 用の `INTERLEAVE_3_0`、`INTERLEAVE_3_1`、`INTERLEAVE_3_2`、`INTERLEAVE_3_4` だけを受け付け、mode 1 / mode 2 用の値は拒否する。
- blind scan は `UNAVAILABLE` とする。

対象 driver は modulation / coderate / guard interval / time interleave を userspace から細かく強制設定するモデルではない。したがって、これらの具体値は「driverへ個別プログラムする knob」ではなく、Android 14 AIDL 上 advertise した運用可能値として検証する。backend は frequency / bandwidth / stream selector を主入力として tune し、demod の自動検出に委ねる。

### ISDB-S validation

- `modulation` は `AUTO`、`MOD_BPSK`、`MOD_QPSK`、`MOD_TC8PSK` だけを受け付ける。
- `coderate` は `AUTO`、`CODERATE_1_2`、`CODERATE_2_3`、`CODERATE_3_4`、`CODERATE_5_6`、`CODERATE_7_8` だけを受け付ける。
- public settings の `symbolRate` は `0` / 未指定相当のみ成功とする。
- BS は `streamId` を必須とする。
- CS110 は stream selector を指定してはならない。
- blind scan は `UNAVAILABLE` とする。

共通 validation は binder 層の `settings_to_request()` に集約し、backend 固有 validation は `Px4FrontendBackend::validate_tune_request()` / `DvbFrontendBackend::validate_tune_request()` を通す。public `tune()` / `scan()` は validation 済み request だけを backend へ渡す。



## worker abnormal exit と scan terminal state の固定方針

worker panic は log-only にしない。`spawn_worker_with_exit_hook()` / `join_worker_with_diagnostics()` が `WorkerExit` を返し、panic は diagnostics と affected object state に反映する。live pump / tune worker / scan worker は panic hook で `record_runtime_failure()` と `mark_live_path_failed()` を実行する。

scan worker は次の terminal reason を保持する。

```text
Running
Completed
Cancelled
FailedBackend
FailedCallback
FailedPanic
```

scan の normal / stopScan / backend error / callback error / panic は区別して diagnostics に残す。callback 登録済みで scan が開始済みの場合、terminal 時に可能な限り END を送る。ただし END 送信は成功扱いを意味しない。
