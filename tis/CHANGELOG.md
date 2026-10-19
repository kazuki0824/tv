# 2026-10-25 入力予算判定の冗長分岐を簡素化（#207）

- Qodanaの`RedundantIf`指摘に対応し、`withinQueueBudget()`のbyte上限比較を直接返す。冗長分岐の導入は#207の`3bb499c3`（PTS span判定の撤去時）で、`54f6e12`で投入中sampleを含む現行比較式へ変更された。件数・byte予算、投入中sampleの会計、境界値を含む受付結果は変更しない。動作を変えない簡素化のため専用試験を追加しない。
- 検証：既存`PlaybackFilterAdmissionTest`44件成功、failure/error/skipは0。ktlint・detekt・`git diff --check`成功。Qodanaの再判定はpush後のCIで確認する。

# 2026-10-25 再開予約をSessionへ集約（#207）

- PipelineのCodec再試行履歴・再開予約・音声縮退の直接再開を削除。入力欠落、recoverable/transient Codec障害、構成・初回出力期限、映像出力障害は旧世代の停止・解放後に元generation付きで通知する。解放失敗は`PLAYBACK_RECOVERY_FAILED`として通知し、再試行可能な障害に混ぜない。ERROR_RECLAIMED・無効Surfaceはtimer再開対象にしない。
- Sessionの予約を一件に統一し、実行・取消時に消費する。既存300ms後にtune・署名・失敗世代・Failed・release・Surface・信号を照合し、通常開始入口とControllerから最新SI/CAS/視聴制限/trackを再評価する。SI更新による待機迂回を防ぐ。音声縮退はSessionが音声を無効化して同じ入口から再開する。Pipelineの開始は既存資源判定を共有し、停止済み世代を重複停止しない。出力先・PCM変更の同期再構成は障害後の再試行と区別して維持する。
- 廃止するPipelineの回数制限・予約helperに依存する通常ホスト試験3件とRobolectric試験1件を削除。残る停止・元世代通知の試験を新契約に合わせ、試験だけの代替製品機構は設けない。CI件数gateはホスト412→409、受付45→44。入力バッチ試験はnative完了との競合を同期し、Surface障害試験は実停止に対応する既存MediaSync shadowを使用する。
- Robolectric CI指定10クラス87件成功（受付44件・MediaSync解放14件を含む）、failure/error/skipは0。`testDebugUnitTest -Pkotlin.incremental=false`で実行。ローカル増分cache破損後は増分コンパイルを無効化した。前段の二度目の再試行資格の失敗は成功へ転じた。予約の2試験は状態境界・実予約処理の検証であり、onTune/onRelease全経路や実Codecの再開成功を証明しない。
- 通常ホスト構成で本番・全ホスト試験Kotlinをコンパイルし、`PlaybackStartGateTest`、`PlaybackResourceCleanupTest`、`PlaybackFailureCallbacksTest`をJUnit実行。49件中43件成功、6件はSI JNIライブラリ不在による実行環境エラー。SIを使うscan/tune試験の成功やホストCI全体の成功とは扱わない。Android製品build・atest/VTS・実機での回復・300msの負荷妥当性は未確認。

| 条件 | 対象・処理経路 | 確認コマンド | 出力要約・未分類ヒット | 判定 |
|---|---|---|---|---|
| 既知NG: 同じ選局の後の障害で資格を失う | `MaleicacidLiveSession.kt`の予約消費→通常開始入口、`PlaybackFilterAdmissionTest.kt` | Gradle CI指定10クラスの`testDebugUnitTest` | 87件成功。`laterRecoverableFailureInSameTuneCanBecomeEligibleAgain`成功。未分類なし | ホスト範囲OK |
| 停止・解放失敗の区別と元世代通知 | `PlaybackPipeline.kt`の`failPlayback`→停止→通知、`PlaybackFailureCallbacksTest.kt` | 上記3クラスのJUnitCore | 音声失敗・再構成・解放失敗の試験成功。別のSI依存6件はJNI不在に分類 | 対象経路OK、全体未確認 |
| 旧履歴・予約・縮退結果fieldの参照残り | 本番、host、tests | `rg -n 'codecRecoveryAttempted|retriedPlaybackSignature|recoverCodecGeneration|codecRecoveryDelay|videoOnly' tis --glob '!**/build/**' --glob '!**/.gradle/**'` | 変更履歴を除く定義・参照0件。全Kotlin型照合成功。未分類なし | OK |
| 製品の再開・最終描画 | `INTEGRATION.md`の集約後再開確認 | 未実行 | 製品build/実機なし | 未確認 |

# 2026-10-25 再試行方針の仕様確定と修正前の回帰試験（#207）

- 再試行判断・予約をSessionへ集約する設計を確定。Pipelineは障害分類と停止・解放、Controllerは現在選局・CAS照合を担当する。停止完了後300msの既存Session待機値を引き継ぎ、通算一回制限を廃止する。300msの実機負荷妥当性は未実証。持続障害では間隔を置いた再試行が続く。保持予算の数値は変更しない。
- 再開時は通常の開始入口で最新の視聴条件を確認する。停止完了通知は失敗元generationへ関連付け、予約はtune・署名・失敗世代・Failed状態・releaseを照合して実行する。未解放資源があれば再開しない。製品実装の反映は後続工程であり、このコミットで完了したとは扱わない。
- 既存Robolectric受付試験に2件追加。同じ選局で別の再生世代が失敗しても再試行を受理する動作と、停止・解放・別選局の状態境界後に予約が状態を上書きしない動作を実Sessionの予約処理で検査する。fixtureは端末初期化を省き、状態境界を設定する。onTune/onRelease全経路、実Codec再開、最終描画の実証ではない。削除予定の再試行履歴フィールドの値や存在は検査しない。
- ローカル実行: `ANDROID_HOME=/tmp/tv42-sdk /tmp/tv42-tools/gradle-8.9/bin/gradle -p tis/host_ci/robolectric testDebugUnitTest --tests '*PlaybackFilterAdmissionTest' --tests '*PlaybackMediaSyncReleaseTest' --offline`。受付45件中44件成功・1件失敗、解放14件成功、error/skipは0。失敗は `laterRecoverableFailureInSameTuneCanBecomeEligibleAgain` の二度目の再試行受付で、Idleを期待するところFailedのまま。既存一回制限による修正前の失敗を固定し、Ignoreや失敗許容で隠さない。CIの受付件数gateを43から45へ更新したため、本段階のCIは当該未修正動作で失敗する。
- 変更Kotlinのktlint・detektと`git diff --check`は成功。Android製品build・atest/VTS・実機再生は未実施。停止・選局との両立は上記状態境界試験と既存寿命管理の静的確認の範囲であり、集約後実装の全経路検証は後続工程に残る。

# 2026-10-24 映像native入力を既存映像workerへ分離（#207）

- `DecoderPipeline`の映像`QueueRequest.queue()`を既存の映像出力Handlerへ配送し、音声返却・制御を担う共通playback ownerをnative入力待機から解放する。追加worker、payloadコピー、別入力queue、容量増加はない。投入中は1呼出しとし、未投入の残りは既存pendingSamplesで保持する。投入中sampleも既存の件数・byte予算へ含める。
- native呼出し結果をFutureでownerへ渡し、入力進捗・再生開始・例外通知・MediaEvent解放をownerに保持する。早着入力index callbackはnative完了後に処理し、成功した入力はindex返却またはCodec解放まで保持する。配送拒否・native失敗・停止による未実行入力の取消しは一度だけ返却する。先行入力の失敗を次sample受理前に取り込む場合も、その未受理sampleを返却する。
- 停止時は新しい入力投入を止め、MediaSync flush後にnative入力・出力の完了を待ち、その後でpause・解放する。停止期限超過時は入力・Codec・MediaSyncを保持し、停止再試行へ残す。停止後の古いCodec callbackと完了通知は再生を進めない。設計書の入力owner・実行worker・停止順序を更新した。
- 工程2の回帰2件に加え、早着index返却、native入力失敗、入力待機中の停止、worker実行前の入力取消し、停止期限超過後の後着callbackと停止再試行の5件を追加した。Robolectric CIの受付試験数gateを38件から43件へ更新する。既存fixtureのdrainは配送済みnative入力の完了も待ち、継続再生試験が未投入indexを再返却しないよう同期する。
- 検証：Gradle 8.9 / Robolectric 4.16.1 / SDK 35でCI指定10クラス86件が成功（入力43件、停止14件を含む）。工程2の失敗2件は成功へ転じた。停止期限超過後の後着callback試験も、世代・停止状態の検査追加前の失敗と追加後の成功を確認した。変更Kotlinのktlint 1.8.0、detekt 1.23.8、`git diff --check`は成功。
- Android製品build、atest/VTS、実機MXワンセグ1での改善は未検証。実機の全容量拒否・黒画面を解消済みとは扱わない。製品統合文書へ入力停滞の再評価条件を追記した。

# 2026-10-24 42系の映像native入力待機を固定する修正前回帰試験（#207）

- `PlaybackFilterAdmissionTest`のCodec native入力shadowへ待機を注入し、本番Filter→decoder→`QueueRequest.queue()`の入力経路を通す。映像native入力が戻らない間にも制御処理と消費済み音声のCodec返却が進むことを別々に要求する2試験を追加した。音声はMediaSync callbackからownerへの引渡し境界を起点とし、native MediaSync自体や実機の表示待機は再現しない。
- 同じ音声所有fixtureで映像native入力が待たない場合の返却成功を対照試験として追加した。待機の到達と解除はラッチで同期し、失敗時も解除してownerを回収する。15秒はfixture終了保証用であり、製品の待機期限・容量・所有規則は変更しない。Robolectric CIの試験数gateを35件から38件へ更新した。
- 根拠はa8d6778の42系MXワンセグ1で、停止前snapshotのplayback ownerが`native_queueLinearBlock`内で待ち、PerfettoのCodec入力待機が約3.191秒、音声consumeのowner待機が約2.936秒だったこと。映像出力workerだけの分離では入力native呼出しによる共通ownerの停止を防げない。容量拒否全体の唯一の原因や永久deadlockと断定しない。
- 検証：Gradle 8.9 / Robolectric 4.16.1 / SDK 35で本番・試験Kotlinをコンパイルし、38件中既存35件と対照1件が成功、新規待機回帰2件だけが制御処理・音声出力返却のassertionで失敗した。試験準備時のCodec一覧キャッシュは新fixture内で破棄して音声Codecを登録する。ktlint 1.8.0、detekt 1.23.8、`git diff --check`は成功。CIも修正前回帰2件の失敗を期待する段階である。
- 今回は修正前回帰を先に追加する段階で、本番実装の修正、Android製品build、atest/VTS、実機での改善確認は行わない。

# 2026-10-23 停止時の後着映像投入とJNI内部TRACE抑止（#207）

- `VideoDecoderPipeline.stopRenderSubmissions`でatomic停止フラグを立て、予約済みの未実行出力は既存workerで非描画返却する。実行中出力をflushで起こして全完了を待ってからMediaSyncをpause/flush・切断・releaseする。フラグ検査を通過済みの呼出しとの競合でも、完了前にpauseしない。既存の停止期限・失敗時の参照保持・再試行・PCM/Codec解放順は維持し、新workerやqueue、容量上限は追加しない。
- 工程2の停止回帰2件に非描画返却件数を追加し、停止判定通過後のnative投入がflushより遅れる競合を追加した。Robolectric CIの試験数gateも11件から14件へ更新した。Android 15実SDKへの本番コンパイルとRobolectric 4.16.1 / SDK 35で停止関連14件が成功。修正前の追加2件は停止期限超過で失敗していた。
- 工程2で追加した`log.tag.jni::wrapper::objects..=D`の存在検査を撤回した。Android標準Rust loggerは書込み前に23バイトへタグを短縮し、LineageOSのproperty名検査は連続・末尾ドットを拒否するため、この設定要求は実現不能だった。短縮前の完全module名やprefixのpropertyでも一致しない。
- SI JNIの標準logger初期化・targetフィルタへ抑止を移し、既存のjnienv propertyと無効な設定要求のCIブロックを除去した。JNI TRACEだけを抑止する実動作試験・Soong/host依存・Rust側設計/履歴は`arib_si_engine_rs`に収録する。`tis/DESIGN_JA.md`の停止順序と`INTEGRATION.md`のログ採取・詳細診断方法を更新した。
- 再生入力・世代・起動・音声切替の`PlaybackFilterAdmissionTest` 35件、Kotlin変更のktlint・detekt、XML/製品統合CI検査は成功。Android製品build、atest/VTS、実機のログ抑止・停止完了・再選局は未検証で、次の製品buildで確認する。音声キュー飽和の初発原因を解消したとは扱わず、同条件のlogcat/Perfettoで再評価する。

# 2026-10-23 40系の停止競合・JNI設定漏れの修正前回帰試験（#207）

- `PlaybackMediaSyncReleaseTest`へ、実行中のnative描画と未実行の2件を残したowner内停止、およびnative worker開始前にflushが先行する停止の2試験を追加した。実際の`PlaybackPipeline.stop()`を呼び、停止完了と停止後の描画再投入なしを要求する。
- 新fixtureは[LineageOS 22.1 MediaSync::flush / onFrameAvailableFromInput](https://github.com/LineageOS/android_frameworks_av/blob/lineage-22.1/media/libstagefright/MediaSync.cpp)の現在の待機だけを解除する振る舞いを模擬する。既存fixtureの一度開くと永久に開いたままのラッチでは検出できなかった後着描画の再待機を検査する。15秒はfixture自身の終了保証用で、製品の5秒待機より先に模擬nativeを成功させないための試験用期限である。製品の期限・資源上限は変更しない。
- 既存TIS host CIの製品設定検査へ`jni::wrapper::objects..`の書込み元VERBOSE抑止を追加した。40系ZIPのlogcatは同タグ845,112行、別添tuner-fixed-40.logでは0行だったため、表示側のフィルタだけでは抑止成功と判定しない。
- 40系はtv `1a6beb6`とtools #18のビルドであり、停止直後のCodec `queueBuffer`は38.050584秒、そのうち約38.01秒はsleep。native入力の待機とownerの`awaitRenderSubmissionsBeforeMediaSyncRelease`が重なる。上流の音声192件飽和とは別の停止競合として扱う。
- 音声は07:59:21.502の入力診断でpending=0、25.719で129、30.798で188、30.971で192件の拒否となった。その間も音声consumeは進行しており、完全停止だけでは説明しない。2 vCPUの約19.778秒区間は約95.82%稼働し、JNI/logcat、HAL、表示処理等が共存する。これらの寄与の一意な分離は未証明で、容量増加や音声経路の推測修正を加えない。
- 検証：本番KotlinをAndroid 15実SDKでコンパイルし、Robolectric 4.16.1 / SDK 35の`JUnitCore com.maleicacid.tvinput.tis.PlaybackMediaSyncReleaseTest`で13件中既存11件成功、追加2件のみ停止期限超過（index=0 / 1）で失敗した。既存host CIのXML・製品設定検査を抽出実行するとobjectsタグ不足で失敗。追加試験のktlint・detektと`git diff --check`は成功した。
- 今回は失敗する試験を先にPushする段階であり、本番実装・製品設定は未修正。停止回帰2件とJNI設定検査は失敗を期待する。実機解消、Android製品build、atest/VTSは未検証。修正後の成功確認は後続の実装修正段階で行う。

# 2026-10-23 映像出力所有順の説明とイベント領域の設計用語（#207）

- PlaybackPipelineのnative render完了・失敗処理、待機期限、資源切断前の終了待ち、失敗時owner保持の4コメントと、診断スクリプトのmediatuner所有の説明を日本語へ変更した。処理・期限・所有順は変更しない。
- SI保持の文書更新を継承し、AV入力保持の説明に残った共有slotという語を現在のイベント専用DMA-BUF領域へ合わせた。queue成功時の早期解放を認めず、Codec入力index返却またはdecoder解放まで保持する既存契約は維持する。

# 2026-10-23 SI保持上限の設計記述の追従（#116）

- SI全体を有限寿命・累積入力上限で破棄する旧記述を削除し、実保持量を増やす入力の拒否、既存の無関係な事実・時計の保持、対象表の矛盾根拠失効へ合わせた。拒否入力を新しい登録・Program削除権限の根拠にしない条件は維持する。
- TISの新しい状態・受付経路は追加しない。Rustの矛盾検出と保持量判定は同モジュールのDESIGN/CHANGELOGを正とする。
# 反復SI入力後の再生継続回帰

- 実JNIにPAT/SDT/NIT/PMTを投入した既存live fixtureで、9,000回の同一PAT反復後もREADYを保ち、その判断を実controllerのCAS/filter更新へ渡してpipeline世代・PMT filterが維持されることを検証する。
- 既存の矛盾PMTによる停止試験を維持し、SI/CASゲートを緩和せず反復入力だけでは停止しないことを区別する。host CIの期待試験数を1件増やした。
- Rustの期限境界試験と組み合わせて全消去の回帰を検出する。実機の長時間視聴、Codec/MediaSync停滞、BS ECM/EMM失敗の原因解消は未検証。

# ライブSI継続保持のJNI試験追従

- 不正SIの反復でcollection全体が失効するという旧期待を置換し、実JNI経由で9,000回の不正入力後もサービス・番組・公開世代が保持されることを検証する。
- live監視の内部寿命の参照先をRust SI設計正本へ変更した。Rustの全消去廃止・EIT表単位の受信断と保持量制御は同モジュールのCHANGELOGを正とする。
- Codec/MediaSync停滞とBS ECM/EMM失敗の原因修正・実機検証は未完了。

# 2026-10-22 39.1のCAS失敗診断とProgram cleanup待機診断

- ECM/EMMのMediaCasStateExceptionについて、公開APIの数値errorCodeとdiagnosticInfoを既存の診断messageへ保持し、messageが空でもplatform側の失敗種別を識別できるようにした。元のcause、CAS状態分類、配送・再試行契約は維持する。
- Program cleanupの非同期開始・実行中はPENDINGと記録し、workerの完了結果だけからCOMPLETED/FAILEDを記録する。Boot EPGとbackground maintenanceの未準備分岐を揃え、非同期開始を実失敗と誤表示しない。
- Framework CAS adapterを通すECM/EMM status保持のホスト試験を追加し、ホストCIの期待件数を401へ更新した。実機のCAS失敗原因・初回スキャンは未検証。

# 2026-10-22 39.1の再生停滞診断

- 入力欠落と再生不能の通常ログに、既存decoder owner上のpending入力、Codec空き入力、未処理入力callback、投入済み入力、保留映像出力、native render未完了数、MediaSync開始状態、未返却音声出力を記録する。native呼出しやrender完了待ちは追加しない。
- native renderを停止させたRobolectric回帰試験から診断を読み、停滞中もplayback ownerが応答し未完了renderを識別できることを検査する。
- 39.1の64件到達を原因そのものとは断定せず、容量・期限・復旧回数を変更しない。Codec/MediaSync内部の実機待機位置とCAS失敗のplatform statusは追加採取が必要であり、この変更だけで症状解消を確認したとは扱わない。

## 2026-10-21 Codec入力通知のFilterバッチ中先行消費（#207）

- Filter一括callback中にCodecのinput-index callbackが到着した場合も、次のイベントの保持予算判定前に同じownerで入力返却とpending drainを実行する。callback threadはindex引渡しのみを行い、Decoder状態とgeneration判定は既存ownerに維持する。
- 現行MPEG-2の48件pending上限や16MiB callback permitを無条件増加させず、不要な人工的破棄を減らす。
- 変更前に49イベントの途中入力通知で48件目以降の人工破棄を確認するRobolectric回帰を追加。実機の低速Codec、永続的な出力停止、メモリ使用量は別途検証する。

## 2026-10-21 R1 native映像出力中も再生ownerを進行させる（#207）

- Native MediaCodec.releaseOutputBuffer(renderTimestampNs)の待機を既存playback ownerとCodec callback HandlerThreadから独立させ、セッション単位の直列HandlerThreadへ配送する。MediaCodecの貸与済み出力indexの寿命が自然な並列上限であり、固定数の追加render pool/出力payloadコピーは設けない。
- render結果は既存ownerで回収し、停止時はMediaSyncをflushしてからnative実行の完了を確認する。期限超過時は停止失敗として所有権を保持し、解放成功を捏造しない。releaseが失敗した場合もshutdownせず回収を再試行できるようにする。
- Android 15 Robolectricで、native描画待機中もcontrol ownerが進行し、停止時のMediaSync/Codecの解放順序を維持することを検査。実Codecと実Surfaceの持続描画は端末検証待ち。

## 2026-10-21 startup前3件制限の撤廃とR3〜R5の回帰 (#207)

- decoder構成前の保持イベントは、Codec別に既存設計で確定するstartup件数・byte予算だけで有界化する。HALの廃止済み8-slot前提から残った3件制限を撤廃し、合法入力を構成ヘッダー未発見だけで人工的に欠落させない。
- 最大16MiB単一AVイベントのcallback受付と複数イベント超過時の拒否を区別し、既存16MiB共有permitを維持する。R4のCodec復旧では既停止generationを二重停止せず、R5のCAS未確定時は再生資源がない状態で世代を進めない。
- Robolectricでstartup予算内の12件保持と起動期限での解放、16MiB/過大callback、復旧の一回停止、CAS反復stopを検証する。実機の処理速度と長時間再生成功は別途検証する。

## 2026-10-19 36系JNI TRACEの限定抑制（#207）

- 36系のPerfetto・native stackで、SI DTOのJNI object構築中に`jni::wrapper::jnienv`のVERBOSEログが大量発生し、2 vCPUでTISとlogdの負荷が顕著だったことを確認。
- Android既定`liblog_rust.dylib.so`のloggerを置き換えたり、`JNI_OnLoad`から全Rust moduleの最大log levelを変更したりせず、product system_ext property `log.tag.jni::wrapper::jnienv=D`で該当タグのVERBOSEだけを通常時に抑止する。必要時は`adb shell setprop log.tag.jni::wrapper::jnienv V`で再有効化し、終了後Dへ復元できる。Java TIS DEBUGと他タグのログは維持する。
- 構成値と診断切替を`tis/INTEGRATION.md`へ記載。製品build・実機でのproperty反映・同条件でのfps/CPU比較は未実施。ログ生成抑制がCodecバッファ停滞の解消を示すものではない。

## 2026-10-19 35.1入力飽和の再評価と原因計測（#207）

- T1: 16MiBのcallback payload共有permitは未処理入口の有限受付枠として維持。HALの保持bytesやCodec内部メモリを実測・代替するものではない。容量拒否では予約KiB・イベント数・payload bytesを診断に残し、拒否が生じる事象そのものを区別できるようにした。
- T2: `QueueBudget`でPTS span（2〜3.5秒）を資源保持量の制限として使うのを除去。35.1の音声は約12KiB／71イベントで、bytes・件数は上限内なのに3秒のPTS離隔だけで圧縮入力が欠落した。PTSは実経過時間ではないため、件数とbytesのみを入力・音声output受付の有限上限とし、PTS spanは拒否時の診断値へ残す。
- T3: callback受付超過・Codec入力受付上限等により**実際に圧縮sampleを解放した場合**はCodec参照pictureをそのまま継続利用せず、従来の1回の世代再生成へ接続する方針を維持。PTSだけで発生した人工的な欠落を無くし、再試行workerや新しい復旧状態は追加しない。設計書の「一度sampleを破棄しても継続」との矛盾を解消。
- T4: 既存playback ownerでcallback→owner到達、Codec queue、入力index返却、出力callback、MediaSync音声consumeの進捗と所要時間をDEBUG採取できるようにした。世代別の入力index再通知・返却・音声consumeは既存`PlaybackDataPathProgress`に追記し、DEBUGの所要時間ログは32/64件ごとの採取とする。新worker、producer queue、時間判定へのfeedbackは導入しない。
- T5: Android 15 Robolectric fixtureで有界キューに対するPTS幅3秒超の少量音声入力、および4秒相当のMPEG-2入力120回・Codec出力／描画callback・入力index返却を検証する回帰を追加。診断counter reset試験も追従した。
- **検証範囲:** 実装・設計・テストの変更をPRへ反映したが、Android製品build、Robolectricの実行、実端末のCodec処理能力・黒画面解消はこの変更だけでは確認できていない。実Codecで入力を消費できない場合はbytes/件数上限に達して失敗し得るため、診断traceと速度比較による原因切分けが残る。toolsの診断PR #17と恒久修正PR #18/19の統合は行わない。

## 2026-10-18 導入元に戻したdual-mono試験の是正

- #142で導入され#116へ統合された、真偽値をそのまま返すisDualMonoSemanticForTestと恒等呼出しを削除。実際のAudioTrack dual-mono mode対応を検査する既存試験は維持する。製品の再生動作は変更しない。

## 2026-10-18 Framework診断差分revertの是正

- 部分revertに混入したtoolsの作業branchへの参照文と「ビルド前の準備」の見出しを取り消す。Framework診断patchとビルド手順の追加差分を消し、新しい統合規定をtvへ加えない。採取手順とTIS自身の診断は維持する。

## 2026-10-18 再生入力欠落とMediaSync停止失敗のレビュー是正

- 構成前でもdecoderの保留入力受理時にOVERFLOW集約を解除し、次の欠落でprobe・保留入力を再回収する。Codec投入進捗による解除を置換する。
- MediaSync release未完了の間は既存ownerにPCM・Codec・入力Surface・AudioTrackを保持する。既存ResourceCleanupの再試行成功後だけ停止の後段へ進み、重複した解放登録を避ける。
- RestartEventの一般則を構成前の局所回収／構成後の全世代再生成と統一する。既存回復機構は変更しない。
- 連続OVERFLOWの既存期待値を修正し、欠落を跨ぐMPEG-2 header誤結合、flush/release継続失敗→成功後のCodec解放の回帰を追加。flush失敗/release成功でもCodec回収まで検査し、CI件数を29+7へ更新する。
- 修正前は新規・更新試験3件が失敗し、修正後はKotlin 1.9.22の製品compileとAndroid 15 Robolectric再生36件が成功。ローカルはキャッシュ済み依存による直接compile/JUnit実行でありGradle実行ではない。Android製品build・atest・VTS・実機症状解消は未実施。

## 2026-10-18 Framework診断差分の配置をrevert

- `0c0116d`で追加したFramework診断patchと、その手動適用・build手順を取り消す。製品ビルド側PR #17がpatch本体と適用を所有し、TIS統合手順は参照だけにする。
- 同commitのMPEG Audio予約値修正・音声PID設計是正・TIS診断・採取scriptは取り消さない。既存Exact-mode patchも今回のrevert対象ではない。
- 製品コード・試験の差分なし。Framework patchの内容一致、tools側の適用・再実行・欠落時の失敗をhostで確認。Android build・atest・VTS・実機確認は未実施。

## 2026-10-18 診断追加に伴うhost compile回帰の是正

- 既存host Log stubへAndroid APIと同じDEBUG値・isLoggableシグネチャを追加。hostでは診断を無効にし、製品のpropertyによるDEBUG制御には変更を加えない。新owner・worker・property模擬機構は追加しない。
- 直前の「Kotlin compile成功」はRobolectricのcompileであり、独立したhost stub構成のcompile成功を意味しなかった。同構成での未定義参照によるCI失敗を認め、検証範囲を訂正する。

## 2026-10-18 MPEG Audio予約値検証と実機診断採取

- #207レビュー2件: 音声PIDのみの切替条件を一箇所へ集約し、PTS/MediaSync全世代再生成の一般則から例外を参照する。実装の局所Filter交換と設計を一致させた。
- 既存`EsHeaderParser.mpegAudioFormat`で予約version/layerを拒否する。CodecHeaderProbe→MediaFormatの回帰で`FF EB 90 64`・`FF F9 90 64`の拒否と、後続の分割された`FF FB 90 64`の正常構成を確認する。新しいparser・判断表は追加しない。
- TISのDEBUG診断へMediaSync生成区間と世代、Codec映像出力の要求/返却、callbackのPTS/受信時刻を追加。availability・資源寿命・回復動作は変更しない。
- Framework用MediaSync診断patchを追加。DEBUG有効時だけnative instance、buffer ID、入力frame、media時刻/表示予定時刻、出力queue結果/次frame番号を記録し、Exact listenerや新しいAPI・保持状態は追加しない。tv checkoutだけではFrameworkへ適用されない。
- Perfetto設定、ホスト採取/snapshot script、INTEGRATIONのビルド/採取/照合手順を追加。native stack、SurfaceFlinger layers/FrameTimeline、外部撮影を組み合わせる。Java stackやqueue成功を最終present成功と扱わず、採取失敗・trace loss・上流画素内容の未確定を明記する。
- 新回帰は修正前に予約versionの誤受理で失敗し、修正後はRobolectric playback 28件＋MediaSync 6件が省略なしで成功。Robolectric構成のKotlin compile、変更範囲ktlint 1.8.0/detekt 1.23.8、Bash構文確認は成功。模擬adbによる採取成功/失敗・property復元・native採取成功/失敗の4ケースを確認した。
- MediaSync原本blob `a3f55da7116afd0b976aec1b23c6917e0578db50`へのpatch適用、既存Exact patchとの両順序適用と結果一致を確認。native Soong build、端末でのPerfetto data source/ログ対応、実native stack採取、実波症状解消は未検証。模擬adbやpatch適用確認をこれらの代替証明としない。

## 2026-10-18 起動前の映像待機とCompatibility時刻比較

- #207: rate 0のMediaSyncへ同期renderして同じowner上の音声準備・開始・停止を塞がないよう、開始前の映像出力はCodecの貸与bufferのまま保持し、既存startup gateで再生開始後に渡す。独自の枠数・期限延長・workerは追加しない。
- Compatibility判定はcallback入口の単調時計の受信時刻とarm時刻を比較する。中間SurfaceのCCodec通知に渡るPTS(ns)と`System.nanoTime()`を比較する誤りを除去した。旧世代・無効Surface・Surface error・Exact modeの拒否条件は維持する。
- 修正前に、PTS/armの比較2件と、pause中の映像出力が開始・停止を塞ぐ2件のRobolectric試験が失敗することを確認。native Codec/MediaSync境界は模擬であり実機再現の証明ではない。
- 修正後検証: `gradle --no-daemon -p tis/host_ci/robolectric testDebugUnitTest --tests '*PlaybackFilterAdmissionTest' --tests '*PlaybackMediaSyncReleaseTest'`で27+6件成功。`LifecycleControlDeadlineTest`はKotlin 1.9.22で本体executor/ResourceCleanupと既存Log stubをコンパイルし、JUnitCoreで20件成功。変更Kotlinのktlint 1.8.0・detekt 1.23.8も成功。Soong/atest/実機は未実施。
- 既存control待機超過の原因例外へownerスレッドのJava stackを添付する。未開始taskの取消しを先に確定し、開始済み操作の結果未確定・元ownerでのcleanupという契約は変更しない。
- 34.1の各黒画面、旧映像の再表示、最後の停止不能をすべて同じ原因と確定したものではない。特に最後の世代は映像Codec生成ログがなく、今回の起動前映像待機だけでは説明できない。実機の症状解消は未確認。

## 2026-10-18 再生失敗と停止位置の診断

- #207: Sessionが受理した再生不能理由・詳細・世代、圧縮入力欠落の発生箇所、停止前の既存AV進捗と停止段階をログへ残す。pending queue拒否では既存の件数・bytes・PTS幅・Codec入力所有数を記録する。
- 34.1（fe7ad3c）の黒画面・旧映像再表示の原因確定用。期限・容量・復旧回数・資源寿命は変更せず、症状の修正完了とは扱わない。実機検証は未実施。

## 圧縮入力欠落後のCodec参照状態の退役

- #207: 構成済みCodecに対するAV OVERFLOW、RestartEvent、owner受付拒否、sample拒否を既存の全再生generation終了・再生成へ接続。Filter.flushだけで旧Codecを継続せず、Codec errorと共通の一回の自動復旧制限を使う。構成前の局所flushは維持する。
- HAL同時8eventという廃止済み前提をstartup保持の説明から除去した。TISにAU parser、PTS補間、別の復旧owner/queueは追加しない。
- 構成済みCodecのoverflowが旧decoder/filterを退役し、再開対象がない場合に元世代へ失敗通知するRobolectric回帰試験へ更新。Android実機・zapping・色の解消は未確認。

## 2026-10-17 MediaSync停止順序とCodec入力寿命

- Android 15のCodec投入用C2Bufferが元MediaEventの解放通知を共有しないため、元event/blockを入力index返却またはdecoder解放成功まで保持し、HAL共有slotの早期再利用を防ぐ。
- NHK停止後の再選局でCodec停止が待機超過したログを受け、MediaSyncのflush・解放をPCM返却とCodec停止より先へ移動。既存ownerとcleanupを使用する。
- 停止順序とflush／解放失敗をRobolectricで検査する。実機の映像破損・AAC入力不足・停止回復は別途確認が必要。

# setup開始拒否時の世代枯渇表示

- 開始拒否時は既存ScanState.Failedの理由を表示し、SCAN_GENERATION_EXHAUSTEDを前scanの終了待ちへ上書きしない。新しい拒否状態・schedulerは追加せず、setup generationを維持する。
- Robolectricで実startIfIdleとActivityの拒否表示を接続し、active scanの待機表示とInt.MAX_VALUE/activeTask=nullの恒久失敗表示を検査する。


# 標準APIによるFrontend pre-closeと再試行

- releaseと異種Frontend切替でcloseFrontend()成功後にだけSDK close()へ進む。一時的なFrontend失敗ではowner/SDKの開状態を維持して同じ入口から再試行する。
- 他資源のcleanup失敗中もSDK closeを保留する。Frontend回収後のDemux/client/native失敗はone-shot未完義務として区別し、再closeのno-opを成功にしない。
- 必要性を実証していなかった独自Framework cleanup patchとreflection入口を撤回し、標準APIだけの経路へ戻した。
- Android15実SDK closeFrontendとnative/TRM Shadow境界で一時失敗・継続失敗・再試行成功・後続scanを検査する。実機HAL隔離/回収と実scanは別途検証を要する。
# ExactからCompatibilityへの降格時の継続監視取消し

- private callbackの再arm不成立時に既存Exact watchdogを取り消し、実行時もcurrent modeを照合する。同じowner/世代上で処理し、Compatibilityの正常なCodec進捗を最終出力timeoutとして停止しない。
- Android15 Robolectricで初回Exact成功から再arm不成立・Compatibility降格・7秒間のCodec callback継続を検査する。初回Exact eventはfixtureで配送し、再armのprivate API不在はstock SDKの実経路を通す。実機検証の代替にはしない。

# 任意MediaSync patch未適用時の近似Compatibility復元

- private callback不在/登録失敗時はcurrent Codecの公開描画callbackを近似available根拠として使用し、最終output成功と区別して診断する。
- generation、Codec identity、arm時刻、Surface有効性とMediaSync Surface errorを検査し、ExactではCodec callbackだけで通知しない。
- Compatibilityは厳密な最終出力watchdogを開始しない。初回callback欠落と最終出力の観測不能を区別する。
- Robolectricへstock本番arm・一度のavailable・旧callback・確認済みSurface異常・Exactの分離を追加した。

# 音声PID切替失敗後の同一選択による復旧

- 新Filter開始成功後に選択を確定し、失敗時は旧選択と既存cleanup義務を保持する。
- Sessionの無条件noopを除去し、実Filter不在なら同一track再選択から音声入力を再取得する。映像とMediaSyncの世代は維持する。
- close/open/configure/start失敗とSession選択・実Filter・再選択復旧をAndroid15 Robolectricで検査する。

# SDK/TRM未完回収とsetup開始拒否の可視化

- one-shot例外だけでActiveScanTaskを除去せず、Framework client/native後始末も完了するまでControllerとexecutorを保持する。公開closeFrontendによるFrontend/TRM lease回収を評価し、残るclient/native義務と区別する。
- LineageOS 22.1へ解放専用retryCloseForCleanupのpatchを追加。成功したclient unregisterは再投入せず、元Tunerを通常利用へ戻さない。TISは追加hidden入口がない構成を解放未確定として扱う。
- setup開始拒否を同期callbackでUIへ通知し、終了待ちを表示して以前のsetup generationを維持する。独立schedulerや自動再開始は追加しない。
- 既存Robolectric/hostの期待をowner保持へ変更し、SDK一回性・Frontend cleanup・拒否通知を検査する。platform patchの適用確認と実機Soong・TRM再取得・VTS・実波は別の検証条件とする。

# Tuner closeと他資源の複合解放失敗の分離

- Controller内の再試行可能な解放失敗が残る間は、Tunerのone-shot close失敗だけに分類せず、既存資源ownerとcontroller executorを保持する。SDK close自体は再実行しない。
- 既存ResourceCleanupを利用するplaybackを含め、他資源の解放完了後だけone-shot失敗を通知してscan受付を解放する。独立reaper・追加queueは設けない。
- Android 15 Robolectric試験へ、実ControllerのSection Filter closeとSDK Tuner closeの同時失敗、継続失敗時のowner保持、後続回収と受付解放、SDK close一回性を追加した。
- 設計書のexecutor停止条件とscan退役条件を同じ契約へ追従した。Android実機Soong・atest・VTS・実放送検証は未実施。

# scan中断のFramework Tuner one-shot close失敗とstop/start復帰診断

- Android 15のTuner.closeは途中で例外を出してもmClosed=trueとなり、再closeしてもnative retryしない。TunerControllerはこの失敗だけをTunerOneShotCloseFailureに型付けする。
- ChannelScanManagerは他の解放対象が完了した場合に限り、この復帰不能SDK instanceをActiveScanTaskから退役させる。失敗を成功へ変換せずログに記録し、HAL workerの後片付けは既存HAL reaperが所有する。
- 新scanのtune同期拒否、generation、候補、terminal outcomeを記録。新TISプロセスでstop/start後に即時終了しても根拠結果が残る。
- ScanPlanPolicyTestにone-shot失敗後のmanager受付開放試験、Robolectricにone-shot typed診断を追加（既存ケースを更新）。
- 実機HAL workerの物理終了およびstop/start後の新scan成功は本変更だけでは検証できない。原因の異なるPX4 EAGAINやsignal lock不成立は結果コード・実波の追跡が必要。

# 最終出力可用性と音声選択の局所切替

- Codec→MediaSync入力callbackによるavailable確定と継続watchdog更新を除去し、既存の最終出力one-shotを再armして継続を観測する。未パッチOSは観測不能を診断し初回期限で失敗する。
- 同一音声再選択をnoopとし、同じvideo/PCR/audio構成でのPID変更は音声Filterだけを切り替える。PCM/codec構成変更では既存のMediaSync全世代再生成を維持する。
- Sessionの再選択判定と成功signatureを追従し、旧Filter入力の拒否、同一構成/PID切替の映像owner保持、Codec入力callbackによる誤available/watchdog更新防止をRobolectricで検査する。
- 設計と統合文書を追従。Android Soong・実機VTS・実デコーダ・実波の検証は未実施。

# ライブ再生のcodec別最小ヘッダprobeへの移行（PR #207）

- ES履歴を256/512/768 KiB保持して繰返し解析する方式を廃止。MPEG-2 Videoはsequence headerに必要な8 byte、MPEG audioは4 byte、AVC/HEVCはconfig用のSPS/PPS/VPS NAL（1件16 KiB以内）、AAC ADTSは構成に必要な1 frame（13-bit frame長以内）だけを有限保持する。
- MediaEventの境界をまたぐstart code/NALを逐次検出し、構成に不要な映像音声ES本文を保持しない。GOP/AU再構成、独自PTS補完、追加ESキューは設けず、元LinearBlockの直接queue契約を維持する。
- 構成前の未queue MediaEventは同時3件以下に抑え、HAL 8-slot受付の進行性を確保。ヘッダ未検出時の失敗期限は既存のdecoder startup deadlineを正本とする。
- MPEG-2先頭1 MiB超のヘッダ探索、AVC/HEVCのパラメータセット跨ぎ、MPEG audio、ADTSのフレーム境界と超過異常のhost回帰6件を追加。RobolectricのMPEG-2 decoder fixtureも修正した。
- Android Soong、現HEADのhost/Robolectric/Quality CI、実波連続再生は未実施・未確認。HEVC/AVC/AACの実codec適合は実機検証まで未確定。

# Section Filter拒否時の現行性再確認

- 容量不足でcontroller data投入を拒否されたSectionは、既存control executorへ非同期で現行性判定を委譲する。閉鎖済みFilterの同一tune generationを新たな喪失として計上せず、現行Filterの拒否だけを既存のinputDeliveryLossに記録する。新しいqueue、worker、状態所有者は設けない。
- Android 15 Robolectricに、同一generationでのFilter閉鎖後の遅延拒否と現行Filter拒否の対比を追加。実機検証は未実施。

# TIS Section Filter overflowの現行性検証とscan候補別の配送喪失診断

- FrameworkのSection Filter `STATUS_OVERFLOW`を、既存`SectionIngestController`へController受付拒否とは独立した診断件数として記録する。callbackは同期controller待機を行わずcontrolへ投入し、Filter実体・tune generation・現行状態の照合後だけ計数する。
- scanの選局開始前に両カウンターの同一snapshotを保持し、候補ごとの増分を出す。再送でSI収集がCOMPLETEになっても候補内の喪失を消さず、過去候補の累積値だけで新候補の喪失を報告しない。
- Android 15 Robolectricの既存Filter fixtureで現行、別Filter、close後、旧generation、DATA_READY、複合statusのcallbackを検査し、ホスト試験でCOMPLETE時の喪失保持と候補境界を検査する。新owner・追加queue・受付容量変更はない。
- Android Soong、device atest、実機VTS、実波scanでのSection喪失低減の確認は実施していない。結果は対象HEADのCIで判定する。

# TIS設計文書の観測履歴分離

- 旧fixed20実波観測では配送損失19,530件中PID 0x12が12,942件であり、SDT-actual受信後破棄も観測した。現行のEIT入力限定とPAT受理時だけのPMT再評価は設計正本に残し、過去観測数とPR #197への変更経緯だけを設計書から削除した。
- 本変更は実装・公開契約・試験数を変えない。実機Soong、VTS、実波は未実施。

# PR #116 MediaSync停止回帰試験のKotlin quality対応

- `PlaybackMediaSyncReleaseTest`の失敗注入をDetekt `UseCheckOrError`に従い`error()`へ整え、Robolectric shadowのsetter引数をktlint 1.8.0の複数引数関数宣言の改行規則へ合わせた。
- 実行契約・TIS本番停止処理を変更しない。直前HEADのRobolectric／TISホスト試験は成功したがKotlin quality CI（detekt/ktlint）が失敗していたため、本コミットで該当箇所を是正する。新HEADの結果は再確認する。

# PR #116 未パッチLineageOSでのMediaSync出力Surface切断

- 再生停止時、旧MediaSyncへ接続した出力Surfaceを`MediaSync.setSurface(null)`で明示的に切断してから`MediaSync.release()`する。解放の責務・実行ownerは既存PlaybackPipelineのままとし、native MediaSync OutputListenerによる参照循環が接続解除を妨げる経路に対処する。
- 既存ResourceCleanupの単一解放義務で切断・解放を実行する。切断に失敗した場合は旧MediaSyncの解放を成功扱いせず同じownerのcleanupを再試行し、新しい再生を開始しない。
- Robolectric SDK 35の`PlaybackMediaSyncReleaseTest`に、停止後の同一Surface再利用および切断失敗後の未解放義務保持・再試行を追加。独自platform patchを前提としない。
- 今回の変更では実際の未パッチLineageOSのCodec初回映像出力、TISクラッシュ後の異常終了復旧、Framework JNIのWeakGlobal欠陥は修正・実証していない。テスト/CI結果は対象HEADでの実行後に判定する。

# PR #116 初回映像availability確定時の成功診断と実Codec callback回帰

- 初回映像を利用可能とする既存の一回限りの確定点（`videoAvailableNotified.compareAndSet`成功時）から、`VIDEO_AVAILABILITY_COMMITTED`を1回だけInfoログへ出力する。現在generation、Exact/Compatibilityの区別、映像・音声別のAVデータ経路snapshotを含め、失効世代・重複callbackでは出力しない。未パッチOSのCompatibility frame callbackを最終display present成功とみなさない。
- 既存RobolectricのAndroid 15 `MediaCodec.Callback`配送とblock model入力を使い、映像Filter受信→Codec入力→非空出力→frame rendered→availability確定時のsnapshot、重複・旧世代callback拒否を試験する。Codec native queue/releaseだけをshadowで置換し、productionにtest用APIは追加しない。
- 音声Codec出力だけがある場合の`AUDIO.CODEC_OUTPUT=1` / `VIDEO.CODEC_OUTPUT=0`、旧世代音声Codec出力の非計上、`FIRST_FRAME_TIMEOUT`と`DECODER_STARTUP_TIMEOUT stage=FIRST_OUTPUT`の診断snapshot、および実`PlaybackPipeline.start()`による新世代でのcounter resetを追加試験する。
- Robolectric試験の固定件数を12→16件へ追従。コミット`480b1dba`の[CI](https://github.com/kazuki0824/tv/actions/runs/38492993564)で16件成功、ktlint整形違反は後続コミットで修正した。最新HEADの全CIは確認を要する。
- 所有者、状態機械、MediaSync/MediaCodecの意味、TIS/HAL公開契約、資源寿命、再生制御を変更しない。実機Codec・MediaSync最終出力・物理display成功とMXワンセグ1／NHK総合1再生は未検証。Rebase・Resolveはしていない。

# PR #116 block model MediaCodec callback の結合回帰試験

- 既存RobolectricのAndroid 15 Filter fixtureにMediaCodec用shadowを接続し、本番VideoDecoderPipelineが登録する実MediaCodec.Callbackを発火する試験を追加する。Video MediaEventの受理からInputBufferAvailable→block model QueueRequest.queue→OutputBufferAvailable→Video onOutput→FrameRenderedListenerへ進む診断値を確認する。
- 別試験ではInputBufferAvailableとQueueRequest投入が成功しても映像OutputBufferAvailableが来なければ、既存first-frame期限からVIDEO.CODEC_OUTPUT=0・VIDEO.FRAME_RENDERED=0を含む失敗通知が出ることを検査する。
- MediaCodecのnative queue/releaseだけをRobolectric shadowに置き換える。製品runtimeの公開API、Codec判定契約、再生状態機械、資源寿命、callback generationは変更しない。実機codec挙動とLineageOS 22.1の最終描画結果は引き続き実機で検証する。
- 後続の音声出力・世代失効・Decoder起動期限の試験を加え、Robolectricの`PlaybackFilterAdmissionTest`は16件とも成功した（[run 38492993564](https://github.com/kazuki0824/tv/actions/runs/38492993564)）。同runではktlint整形違反があり、後続コミットで整形を是正した。最終HEADでの全CI確認は別途必要。

# PR #116 再生診断レビュー指摘の是正

- 再生progressを同一owner内の映像・音声別counterへ分離し、音声Decoderだけに非空出力がある状況を映像出力成功と誤認しないようにした。再生制御、MediaCodec callback、resource lifetimeの契約は変更しない。
- MediaEvent受信と有効sample計数を各input identityの現行性確認後へ移動し、OVERFLOW/flush・再選局で失効した旧入力の現行進捗誤計上を防止した。
- Filter.flush失敗を音声・映像別のFLUSH_FAILEDとして記録し、失敗通知へ同時点のprogress snapshotを含める。
- progress値型の音声/映像別・reset試験に加え、既存Robolectricの実Filter callback経路で旧入力失効、正常入力受理、OVERFLOW、flush失敗通知、復帰の診断値を検証する。後続のCodec callback・期限・世代reset統合試験は上段に記載する。Soong/実機は未実施。
- レビューの4件へスレッドで個別返信。Rebase・Resolveは実施しない。

# PR #116 黒画面解析の再生進捗診断

- 既存playback executor上の`PlaybackDataPathProgress`にMediaEvent受理、正常sample、Decoder入力・出力、描画callback、OVERFLOW、flush成否、LinearBlock欠落・入力拒否を計数する診断専用counterを追加し、playback世代の開始時に初期化する。first-frameとDecoder起動期限の失敗detailへsnapshotを接続し、再生可否判定やAV資源寿命には用いない。
- 進捗counterの更新、初期未到達値、世代resetを回帰試験で固定。既存sample解析のComplexMethodを増やさず、診断のための分岐により発生したKotlin整形とDetektの指摘を修正した。
- 診断用fieldを追加した後、従来のUnsafe.allocateInstanceで本番constructorを省くPlaybackFailureCallbacksTest fixtureが未初期化fieldを参照して3試験を失敗させていたため、fixtureで同fieldを明示初期化する。testを除外せず、本番ロジックへnull fallbackを入れない。
- テストクラス追加に伴いTISホストCIの固定期待値を49クラス（実行45クラス）・398試験へ更新する。追加時の旧固定値48/44/397に起因する失敗を除去し、後続CIで398件の成功を確認した。
- MediaSync最終出力patchを当てないAndroid 15/LineageOS 22.1を前提とし、実機でMXワンセグ1とNHK総合1の映像表示・持続再生・再選局を確認する必要がある。黒画面の根本原因確定、Soong/device atest/VTS、実機での映像回復は未達。

# 非待機playback配送の容量拒否と実行失敗の分離

- 64 callback固定の受付を、既存AV Filter容量に基づくpayload/event帳簿の有限予約へ変更する。同じSemaphoreのweighted acquire/releaseを使用し、owner再入とshutdown破棄でも全予約を返す。
- 容量拒否はAVの既存decoder backpressure期限、字幕の既存continuity lost/flushへ接続する。損失通知は各Filterにつき既存controlへ一つだけ保留し、stale generation/Filterの入力はcurrent損失へ読み替えない。非overflow statusはdata枠を消費しない。真の投入/実行失敗のterminal cleanupは維持する。
- 実SDK Filter callback lock、MediaEvent解放、Android時計を使う回帰をRobolectricへ追加し、65件video/audio/字幕/status burst、単発拒否後の再開、継続拒否の期限、字幕の局所回収、stale入力、実行失敗を検査する。weighted permit寿命は既存host試験で確認する。
- 新owner・scheduler・retry loop・SI意味parserは追加しない。build/testは既存CIへ委任し、Soong/device atest/実機VTS/実波は未実施。

# 非待機section配送の有限burst受理

- 1件固定のdata枠を、既存64KiB Filter容量と最大section長から導く16 slotへ変更する。残る拒否は既存ingest診断へ保持し、収集完了時も入力喪失を隠さない。callback待機・第二worker・SI意味解析/coalescingは追加しない。
- 本番Filter callback/read/実JNIへの複数section配送、飽和診断、世代失効の回帰を既存host fixtureへ追加する。検証は既存CIへ委任し、実機VTS・実波は未実施。

# 確立済みlive SI失効後の旧再生停止

- 初回SI待ちの早期returnを、既存Idle状態かつ直前もPENDINGの場合へ限定する。READYからPENDINGへの失効は既存CAS解除・再生停止・字幕終了・利用不能通知へ戻す。新owner・状態機械・migrationは追加しない。
- 初回bootstrapと失効回帰で実JNI fixtureを共用し、CRC付き同一版矛盾PMT後のStopped、pipeline世代失効、CAS plugin退役、ECM/EMM空集合とPMT継続、利用不能通知を検査する。build・試験は既存CIへ委任し、実機VTS・実波は未実施。

# Lifecycle同期control破棄時の終了通知

- callControlの通常control/cleanup control両経路へ既存QueuedTaskの破棄callbackを接続し、未開始FutureをcancelBeforeStartで完了する。開始phase CASと開始済み結果未確定の契約を維持し、terminal cleanup後の取消しをtimeoutとして報告しない。
- 既存host試験へ、owner停止中にterminal cleanupと同期callerを投入し、shutdown後に通常/cleanup双方のcallerがtimeout前にCancellationExceptionで終了する回帰を追加する。別owner・worker・本番待機機構・migrationは追加しない。
- build・試験は既存CIへ委任し、Soong/device atest/実機VTS/実波は未実施。

# Program cleanup中の要求保留の用語統一

- cleanup未完了中の「開始を拒否」を、受理済みlive/setup/EPG要求の「処理開始を保留」へ訂正する。要求継続の既存契約を参照し、要求自体の拒否や旧Program利用と混同しない。実装変更・migration・新ownerは追加しない。
- 文書のみを更新し、Soong/device atest/実機VTS/実波は未実施。

# 優先controller releaseによる未実行Futureの終了通知

- 通常controlとcleanup controlのFutureへ既存QueuedTaskの破棄callbackを接続し、shutdownNowで未実行taskを破棄した場合はcancel完了を通知する。優先releaseが受理済み通常controlを追い越しても同期callerを待機中のまま残さない。
- 既存host境界試験へ本番currentGeneration/releaseの競合と、通常/cleanup両Futureの破棄完了を追加する。解放用fixtureは試験内で共用し、設計とCI検出件数を更新する。別worker・owner・待機loopを本番へ追加しない。
- build・試験は既存CIへ委任し、Soong/device atest/実機VTS/実波は未実施。

# controller releaseの枯渇後cleanup入口

- TunerController.releaseを既存PrioritySerialExecutorのenqueueCleanupへ接続し、通常task sequence枯渇後も同じownerで解放・失敗後の再試行を実行する。通常taskの拒否、成功後だけのshutdownは維持する。
- cleanup classの既存定数を共通queueへ集約し、既存Future投入へcleanup選択を追加する。本番release入口の枯渇・cleanup失敗・再試行・完了を既存host境界試験へ追加し、TIS設計とCI検出件数を更新する。別executor・資源owner・retry loopは追加しない。
- build・試験は既存CIへ委任し、Soong/device atest/実機VTS/実波は未実施。

# 字幕時計確定時のpresentation horizon再検査

- clock未確定中に受理した字幕も、時計確定・再arm時と新規入力の容量判定前に既存60秒horizonを再検査する。超過Displayと従属Clearを診断付きで除去し、既存queueの容量を戻す。
- 既存capacity試験へnullから停止clockへ遷移する64件の反例、通常字幕の再受理、Clearだけが上限を超えるpairの除去と60秒境界の保持を追加する。TIS設計と既存host CIの試験件数を更新する。owner・queue・scheduler・budget・本番test APIを追加しない。
- build・試験は既存CIへ委任し、Soong/device atest/実機VTS/実波は未実施。

# BS探索のSDK scan登録解放とAndroid 15境界試験

- 停止通知後も残るTuner SDKのscan callback登録をcancelScanningで解除し、同じTuner/frontend lease上の次RF探索を可能にする。STOPPEDの受信結果とstream IDsは保持し、停止済み状態に限りnativeのINVALID_STATEを受理する。未停止operationと他の失敗では既存ownerとcleanup再試行を維持する。
- TIS設計と停止済みcleanupの単体期待値をSDK登録寿命へ合わせる。Android 15の実scan/cancelScanning/onScanStoppedと本番operationを接続するRobolectric試験を追加し、constructorのnative/TRM接続とnative呼出しだけを試験境界で代替する。未解放登録による次RF拒否と、SUCCESS/INVALID_STATE後の次RF到達を検査する。
- 既存Robolectric CIへ試験を接続し、host-only source setとKotlin build ownership検査へ登録する。SDK非公開型の試験runtime依存を明示し、失敗時の完全な例外出力と境界試験2件の実行report確認を接続する。新しい本番owner・wrapper・状態・scheduler・migrationは追加しない。buildと試験はCIで確認し、Soong/device atest/実機VTS/実波は未実施。

# PR #175 一括照会契約への統一と未使用結果fieldの除去

- ChannelStoreを既存のindexExistingChannelIds契約へ揃え、呼出元のない単一key Provider照会とtest用fallbackを削除。fixtureも一括indexを直接供給する。
- ProgramUpsertOutcomeの未使用updatedを削除し、既存requestによるinsert/update計数とprogramId=nullの失敗契約を維持する。adapter・互換helper・validatorは追加しない。
- 検証は既存CIへ委任し、Soong/device atest/VTS/実波は未実施。

# 2026-10-09 Program normalize未使用APIの削除

- 本番呼出元のないProgram normalizeのKotlin facade・native宣言・JNI export・Rust公開関数を削除し、設計のAPI一覧も揃えた。
- 既存の境界試験は本番Program builder・key抽出・共通validationへ接続し、Program更新時のcleanup・再収集とkey抽出に必要な内部helperを維持した。互換migrationや代替ownerは追加しない。
- ローカルは差分・参照・整形を確認し、buildと試験は既存CIで検証する。Soong/device atest/VTS/実波は未実施。

# Program cleanup完了通知の例外境界統一

- ready済みの同期通知とworker完了通知を同じ狭いnotifyCompletionへ集約し、通知先の例外を診断へ残す。cleanup成功状態と後続通知は維持する。
- 既存worker失敗・retry試験へ非同期通知例外とready済み通知例外・次の通知継続を追加。設計追従、テストはCIへ委任。
- worker・state machine・scheduler・migrationは追加しない。実機VTSは未実施。

# 2026-10-09 PR186レビュー履歴の整理

- playback登録解除、caption/renderer cleanup保持、Program cleanup要求継続の履歴を古いreleaseより前へ統合した。PMT・cleanup FIFO・音声callback・保留tuneの追加試験を含む通常host検出数は376件。検証はCIへ委ね、Soong/VTS/実機確認は未実施。

# cleanup要求継続と遅延音声callbackの保護

- Program cleanup完了を既存session control・setup scan・boot jobへ通知し、最初の利用要求を失わない。失敗時は旧Program使用を拒否する。単一worker、既存latest tune/ActiveScanTask/pending jobを再利用し、schedulerやmigrationは追加しない。
- MediaSync consume入口はsync/世代照合をmap変更の前に行う。同一sync内の音声IDは再利用せず、最大値で明示失敗にする。新sync作成時だけIDを再開する。
- 遅い実Providerの失敗通知とsetup受付、単一onTune要求の完了後再開/失敗時拒否、最大IDと旧sync callbackによる現在outputの無変更をCI回帰試験へ接続した。検証はCIへ委ね、Soong/VTS/実機確認は未実施。

# PMT初期待ちとProgram IPC境界のレビュー対応

- live SIがPENDINGでも現行世代のPMT Filter取得を進め、playback/Program/CAS確定は既存READY契約まで待機する。実PAT/SDT/NITから本番refresh・Filter開始・PMT受信・READYを通すhost試験を追加した。
- Program batchを64件およびParcel実測bytesで分割する。Android公開IPC推奨値の半分を予算とし、単一operation超過は全batchの書き込み前に失敗させる。provider-dataの切捨てやKotlin側の第二schemaは追加しない。異なる予算・24KB×64行・単一超過のRobolectric試験をCIへ接続した。
- 未実装のservice全体Program index取得は空集合成功ではなく失敗とし、空EITをwriter公開入口で誤commitしないことを既存試験へ追加した。
- 未使用の同期選局恒等helperとbooleanだけの試験を削除し、本番候補loop・tune拒否・TUNE_REJECTED終端と後続候補停止の回帰試験へ置き換えた。
- Controller/Lifecycle executorのqueue・owner識別・permit受理/完了/破棄をPrioritySerialExecutorへ共通化した。既存owner/thread数と各executorの期限・再投入・callback解放方針を維持する。
- ホストJUnit検出数をPMT試験1件分更新し、最上位PR186で373件とする。ローカルでは整形・lint・差分を確認し、テスト実行はCIへ委ねる。Soong/VTS/実機検証は未実施。

# callback実行失敗と複数MediaEventの解放保持

- TvProviderの単一/一覧チャンネル照会のnull cursor診断を日本語へ統一した。失敗を空結果の成功に置き換えない。
- LifecycleSerialExecutorで開始済みcallbackのRuntimeExceptionを既存onFailureへ渡し、失敗通知・release fence・同一ownerのterminal cleanupへ接続した。開始済み入力へonDiscardを重ねない。
- AV callback配列の途中例外では現在の未移譲eventと残余eventを既存releaseMediaEventへ渡す。処理済み/decoder所有eventは再解放せず、解放失敗は既存ResourceCleanupが保持する。
- 恒久設計を投入・実行失敗と配列残余回収へ追従した。既存executor試験を実行開始後の失敗まで拡張し、実AV callback入口への3イベント入力と残余解放retryの試験・ホスト境界fixtureを追加した。CIの検出期待値はPR185で360、PR186で372に更新した。
- ローカルでは整形・lint・差分を確認し、テスト実行はCIへ委ねる。Soong/VTS/実機検証は未実施。

- Move upgrade Program cleanup I/O to its single owner worker; keep live/setup/EPG admission closed until success and retry on the next request after failure. Test blocked ContentResolver, 10,000 rows, partial delete and retry without migration.

- Update host test discovery guards for the additional caption cleanup test class and two tests.

- Keep caption cleanup retry authority until cleanup succeeds; fence terminal presentation without allocating a new epoch.
- Preserve the native renderer handle until its release call succeeds, and localize Program cleanup query failures.

# playback cleanup完了後の登録解除

- requireCleanupComplete成功後にglobal playback登録を解除し、その後generation failureを再throwする。cleanup未完了とgeneration枯渇の組合せでは登録を保持してscan admissionを防ぎ、cleanup retry成功後に解除する試験を追加した。

# MediaEvent解放失敗時の入力停止

- ResourceCleanupの解放結果をBooleanで返し、MediaEventの初回失敗を既存playback terminal cleanupへ接続した。released fenceで新規AV処理を止め、同じownerでFilterをcloseして配送解除する。
- 既存複数event試験を連続release失敗・Filter close失敗・100回の配送非受理と保持数不変・成功後の再試行完了へ拡張した。host境界のFilter callback解除順をAOSPに合わせた。
- 解放義務を捨てず、親codec完了契約を維持する。別owner・scheduler・migrationは追加しない。設計追従、テストはCIへ委任。実機VTS・実波は未実施。

# 2026-10-09 PR185レビュー履歴の整理

- stale/current output解放、codec親子cleanup、AV配列の残余回収の履歴を古いreleaseより前へ統合した。既存実装・試験を維持し、検証はCIへ委ねる。Soong/VTS/実機確認は未実施。

- Localize playback runtime diagnostics while preserving reason codes and cleanup ordering.

- #185: playback recoveryのruntime例外境界へ理由付き局所抑制を配置し、親PR自身のdetekt gateを満たす。current output境界の既存抑制は維持。

## レビュー対応: MediaCodec親子cleanupの確定

- codec release成功時に同codec配下のoutput解放義務とaudio参照を完了し、閉鎖済みcodecへのretryを残さない。
- pending actionに既存codec ownerを紐付け、retry中に親成功で完了した子actionも再実行しない。
- output失敗→parent成功→retry、parentも初回失敗する順序の反例を固定する。

## レビュー対応: current decoder output失敗の通知

- current outputのruntime例外を既存onDecoderFailure/errorSinkへ渡し、data workerからの未捕捉終了を防ぐ。
- cleanup所有を保持したままSessionへtyped failureを通知しdata taskが完了する回帰試験を追加。stale出力は通知せず所有保持を継続。

# MediaCodec出力buffer全release経路の回収所有権

- stale callbackに加え、current videoの空buffer・Surface拒否・時刻指定render、current audioの空buffer・LinearBlock欠落・range拒否・backpressure拒否も共通の既存ResourceCleanupへ接続した。失敗時はclosureがcodec/indexを保持し、current処理では解放完了扱いせず既存失敗通知へ伝播する。audio outstanding集合の解放成功まで保持する経路は維持した。

# stale decoder outputの回収所有権保持

- generation/codec identity不一致の遅延callbackも既存ResourceCleanupへ接続し、release失敗を保持してstop/release時にretryする。audio outstanding集合へ登録前のbufferも回収対象にし、新しいcleanup ownerは追加しない。

# serial executorの到達しないcontrol分類の削除

- 両executorの未使用ControlTask markerとexecute振分けを削除し、controlは既存の明示入口、executeはdataに限定した。
- controllerのFuture専用classを既存FutureTaskへ簡素化。Lifecycleの開始前取消し・開始後結果不明、共通queue・owner・permitは維持する。
- 設計を追従し、既存回帰試験はCIへ委任。新しいowner・framework・migrationは追加しない。実機VTSは未実施。

# cleanup優先classのFIFO保持

- cleanup commandへ通常taskとは独立したchecked sequenceを同じexecutor内で付け、通常identity枯渇時もA/B/Cの投入順を保持する。最大値では明示失敗とし、wrap/reuseはしない。
- ownerをブロックして複数cleanupをenqueueした後の実行順を既存host fixtureへ追加した。別executor・scheduler・台帳は追加しない。テストはCIへ委ね、Soong/VTS/実機確認は未実施。

# 同一PTS置換の従属Clear回収

- 置換対象DisplayのframeTokenに従属するClearも容量計算と成功時commitの除去対象に含める。拒否時は既存queueを保持し、別tokenのClearは変更しない。
- 既存字幕2試験へduration付き同一PTSのbudget＋1回の受理とtoken・件数・bytesの確認を追加する。試験実行はCIへ委ねる。

# 受理済み字幕置換と未実行event解放の登録保持

- 同一PTS字幕は容量検査とtoken確保の成功後に旧Displayを削除し、境界数とbytesの検査結果を実際のqueueへ反映する。
- Filter close後の未実行event回収を停止処理内へ移し、解放失敗中はglobal再生登録と既存cleanup ownerを保持する。
- 既存host試験に受理済み置換のbudget+1回連続投入と、解放失敗・再試行時のglobal登録を追加した。検証はCIで実施する。

# Filter callback容量拒否と字幕置換の所有保持

- LifecycleSerialExecutorのdata投入を待機なしの有限受理/拒否にし、Framework callback lockと優先closeの循環待ちを除去した。
- playbackのAV/PES callbackは即時入口から同じownerへ渡す。拒否・失効・未実行破棄のMediaEventを解放し、失敗は既存ResourceCleanupへ保持する。Filter close後の未実行event回収と解放完了をexecutor停止の前提にした。別queue・cleanup ownerは追加しない。
- 同一PTS字幕の置換後容量を無変更で検査し、token確保後だけqueueへcommitする。拒否時に既存Display/Clearを保持する。
- 実Android 15 Filter lockの64枠飽和、callback資源破棄と解放retry、字幕bytes/境界数拒否をCI回帰試験へ接続する。Soong/VTS/実機試験は未実施。

- accepted tune初期化失敗を既存session release fenceとResourceCleanupへ接続し、旧playbackを停止する。解放失敗と通知失敗は主原因へ保持し、同じownerによる再試行を残す。

- #183: current callback投入失敗は原因を保持してterminal fence・診断・同一owner解放へ接続。通常identity枯渇後もcleanup専用controlで再試行し、shutdown競合だけを無視する。

# lifecycle worker交換とcontrol再入dataのdrain

- beforeExecuteで現worker identityへ更新する。全owner taskのfinishからdeferred dataをdrainし、control→data再入もdata側tailへ配送する。control完了時は空きpermitをtryAcquireし、満杯なら既存data完了に回収を委ねてownerを待機させない。交換workerとcontrol→dataの反例試験を追加した。

# shutdown時のdeferred dataとpermit回収

- discard callbackを通常finishから分離し、shutdown時は再enqueueしない。既存deferred queueを同じmonitorで排他回収し、実行中dataとqueued dataのpermitを一度ずつ返す。別thread shutdownとowner再入の競合試験を追加した。

# PR #119 channel挿入直後のrollback ID引渡し

- 未初期化native ownerを使用する例外注入試験をhost専用ソースへ置き、Android/SoongとRobolectric共用ソースにsun.misc.Unsafe依存を持ち込まない。

- 挿入成功時に既存scanのrollback集合へIDを直接渡し、upsert途中と後続Program snapshot取得の例外でも新規行を既存finalizationで削除可能にした。別owner・scheduler・migrationは追加しない。回帰試験は既存CIへ委ね、Soong/VTS/実機は未実施。

# 同一publicationの既存Program重複key拒否

- existing/new分岐前に全program keyを検査し、重複時はservice batch全体を書き込まない。既存rowありの重複入力試験を追加した。新しいownerやcacheは追加しない。

- レビュー再確認により、現行製品の必要条件を立証できないchannel型移行transactionを撤去した。通常rescanは既存IDとユーザー可視性を保持し、immutable列をupdateから除外する。

## レビュー対応: immutable型を保つupsertの必須semantics

- 型を捨てるdefault updateを未対応failureへ変更し、storeごとに型比較と再作成を明示する。
- test storeも既存型の保持・型不一致時のhidden再作成を実装し、未対応defaultは書込みを開始しない。

## レビュー対応: 必須pending問い合わせのfail-closed

- 未対応ChannelStoreのpending問い合わせdefaultを失敗へ変更し、既存pending rowなしと混同しない。
- 必要なtest storeだけ明示query実装とし、default未実装では書込みなしで失敗する反例を固定する。

# 既存channelのone-seg方式移行

- 型変更時だけProvider batchで新規row作成と旧row削除を行い、通常updateでimmutable COLUMN_TYPEを変更しない。one-segはhidden/non-pendingへ再投影する。upgradeと再作成失敗の反例試験を追加した。

# scan開始時の取消し保持

- initial/maintenance scan入口の不要なcancelled=false代入を除去し、別threadで受付済みのcancelを上書きしない。task生成時の初期化と既存fence/loop/publication gateを維持する。
- 既存取消し・公開gate試験の実行はCIへ委ねる。新しい状態・lock・試験fixtureは追加しない。

- Keep the JVM-only blocking-provider cancellation fixture in the host test target; update class discovery guards without changing test coverage.

- scan取消し受付をProvider公開lockから分離し、最終commit前の受付確定とUIの有限returnを維持。公開中・最終commit中の競合試験を追加。

- Localize the background channel maintenance diagnostic prefix.

## レビュー対応: scan terminalの単一正本

- 終端確定をpublication fenceへ接続し、確定後cancelを拒否する。Managerは正常returnのtyped terminalだけを写像する。
- commit後の遅延cancelがCOMPLETEDを変更しない反例を固定する。

# Program単一行の正常境界とIPC予算の整合

- 推奨IPCサイズと同値の不要local変数を除去し、Qodana UnnecessaryVariableを解消する。予算・分割・拒否条件は変更しない。
- AOSP公開推奨IPCサイズをrequest全体の予算として使用し、追加の半分制限を削除した。件数64とParcel実測の分割、全件事前計測、巨大単一行の明示拒否は維持する。
- 既存Robolectric試験へ32 KiB provider-dataと標準列の受理を追加し、設計を追従させた。Rust schema・切詰め・retry・ownerは追加しない。
- テストはCIへ委任。実機Binder・VTS・実波は未実施。

## レビュー対応: Program bulk/batch正規契約

- 旧単数Program insert/update/genre読戻しAPIとoverrideを削除し、未対応bulk/batch storeをfail-closedにする。
- test storeも64操作単位でstage/commitし、batch途中失敗で先行操作が残らない反例を固定する。

- レビュー対応: authoritative windowのobsolete Program削除未実装をfailureへ変更。必要test storeだけwindow削除を明示実装。

## レビュー対応: 未解決service_type

- 未解決と解決済み非対応を分離し、SDT/NIT収集中のlive policyをPENDINGのまま再評価へ残す。
- null service_typeと未取得SDT/NIT、解決後READY、解決済み非対応UNSUPPORTEDを回帰試験で固定する。

## レビュー対応: scan cancelの公開境界

- cancel確定とTvProvider公開を既存scan ownerのpublication lockへ直列化し、最終snapshot retry中の取消しもCANCELLEDで終了する。
- retry中・publish gate直前の取消しを副作用なしの回帰試験で固定する。

# controller worker交換後のowner判定

- beforeExecuteで現在実行するworker identityへ更新する。未捕捉例外による交換後も単一thread ownerとcontroller/data再入を維持する試験を追加した。thread数やexecutorは増やさない。

- Localize TunerController runtime failure details without changing scan outcomes.

# Program batch後の不要なindex更新の削除

- 読み手のない既存Program index更新と、そのためだけのPendingWrite 3 field・mutable map/list変換を削除した。service単位query、個別event guard、重複key拒否、batch結果/genre readback契約は維持する。
- 既存Program公開回帰試験はCIで確認する。新しい補助処理・migration・owner・試験は追加しない。

# PR #176 非対応SMDのterminal分類

- 確定したUNSUPPORTED_BROADCAST_SYSTEMを既存typed policyのUNSUPPORTEDへ写像する。未取得SMDはPENDINGを維持し、既存試験で両者を区別する。新しい状態・owner・診断文字列分岐は追加しない。検証は既存CIへ委ね、Soong/VTS/実機は未実施。

# controller data入口とshutdown試験の整理

- 本番未使用のsubmitDataを削除し、既存backpressure試験をexecuteDataと試験内latchへ接続した。
- shutdown前の飽和拒否と未実行task破棄後のSemaphore枠返却を別々に直接観測する。
- production owner・permit管理・公開契約は追加しない。テスト実行はCIへ委任し、実機VTS・実波は未実施。

# Filter callbackとcontroller closeの循環待ち解消

- SectionEventはcallback入口でdrainし、parser更新は既存controllerへ非同期・有限・待機なしで投入する。飽和は診断付き拒否とし、世代・Filter identity fenceと順序を維持する。
- Android 15の実Filter callback lockと優先closeを競合させるホスト試験をCIへ追加する。試験実行はCIに委ね、Soong/VTS/実機適合は未確認。

# setup可視化batchの件数不一致をtransaction内で拒否

- 初期可視化の各updateへexpected count 1を指定し、対象行消失をProvider transactionのcommit前に失敗させる。既存pending行のhidden状態とmarkerを保持し、今回insertした行だけを既存rollback入口で削除する。
- 実ContentProviderOperationとSQLite transactionを使うRobolectric試験を追加し、中間・末尾行の消失、既存行と新規行の混在、正常commitを検査する。製品の新しいowner・retry・migrationは追加しない。
- Kotlin build ownership検査へRobolectric専用試験のsource setを登録し、通常host・製品sourceへ混入させずに型照合・実行・品質検査を行う。

# stacked PR直線化時のレビュー残件

- #186レビュー本文で指摘された既存channel一覧queryの未実装defaultを、空集合成功ではなくUnsupportedOperationExceptionのResult.failureへ変更した。writer公開入口で未実装失敗と明示実装の取得結果を既存試験へ追加した。productionの所有input別query、boot/backgroundの失敗伝播、試験数は維持。Soong/device atest/VTS/実機確認は未実施。
- #183の字幕投入失敗境界は、診断通知が失敗しても同じownerでcloseを実行し、双方失敗時には未完解放の例外を記録する順序を維持する。useへの変換は一次例外を変えるため、理由付き局所抑制でQodanaに意図を明示した。処理・所有権・試験数は不変。Soong/device atest/VTS/実機確認は未実施。

- Qodanaの追加4件は取消しCASおよびsurface/track選択のrelease判定を短絡評価へ同値変換して対処し、失効時の後続処理拒否と評価順を保持した。

- #166の既存試験で使用するreflection helperを試験class内へ復元し、controller専用試験との重複は再導入しない。
- #183のlive初期判定はservice観測有無に依存させず、typed PENDINGを待機として保持する。PMT/PCR未完成とterminal stateの反例を境界試験へ追加した。
- 未使用のLifecycleSerialExecutor.submitControlを削除し、試験は既存executeControl/callControlへ接続した。
- queued→runningのCASとSI更新時のrelease再確認を維持し、取消し競合・投入後解放の理由を付けてRedundantIfを対象式だけ抑制した。
- #161からの両系統を各PRの差分を保持して直線化し、setup/one-segの変更をlive owner以降にも継承した。


# setup scanの新規channel可視性transaction

- 実機でscan後52 channelがTvProviderへ存在する一方、大半が`browsable=0`のためstock Live TVの一覧に出ない事象を、setup終了時の初期可視性commit欠落として修正した。
- 通常channel新規rowは`COLUMN_INTERNAL_PROVIDER_FLAG1=1`かつhiddenでstagingし、setup terminalが`COMPLETED`のときだけ、そのscanで正常に再確認したpending rowを`browsable=1, flag1=0`へcommitする。one-segは`TYPE_1SEG`、flag1=0、browsable=0を維持する。
- 既存確定rowのbrowsableは再scanで変更しない。process終了等でfinalizeされなかったpending rowは次の正常setupで同serviceを再確認した場合に回収する。
- setup失敗/cancel/resource-lost/tune-rejectedでは今回新規insertしたrowだけをrollbackする。可視化commit失敗も`INTERNAL_FAILURE`として扱い、今回新規insert分をrollbackする。
- `ACCESS_ALL_EPG_DATA`はplatform署名priv-appへ付与するが、実装上の利用を自package channelの初期可視性確定へ限定し、既存queryは所有row URI制約を維持する。
- 通常/one-segのcommit差、既存hidden保持、失敗rollback、commit failure rollback、process再起動後pending回収の回帰試験を追加した。

# 不要な例示設定の削除

# 不要な例示設定の削除

- 正式な製品組込み設定を継承するだけだった未参照の例示makefileを削除した。

# PR #108 TvProvider queryを公開URI制約へ修正

- `ACCESS_ALL_EPG_DATA`を持たないTISがraw SQL `selection`を渡し、AOSP TvProviderから `SecurityException: Selection not allowed` を受けていた経路を除去した。追加の特権permissionは付与しない。
- channel検索・一覧は `TvContract.buildChannelsUriForInput(inputId)` を使い、service identityの追加条件は返却cursor上で照合する。programのchannel/window検索、廃止行確認、現在番組resolverは `buildProgramsUriForChannel(...)` のchannel/time URIを使用する。
- query failureをempty resultへ丸めない既存契約、program overlapのsort order、provider-data所有確認、更新/削除のrow URIは維持する。Rebaseは実施していない。
- Android/Soong全体、Robolectric/host test、実機Boot EPG syncはpush後CI/実機で確認する。

# PR #108 MediaSync private拡張の任意利用を復活

- MediaSyncがcurrent output Surfaceへの`queueBuffer()`に成功した最初のvideo frameを通知する、LineageOS 22.1向けの最小private拡張patchを復活した。late-drop、attach失敗、queue失敗では通知せず、one-shot armと`armSequence`だけを追加する。
- TISはprivate listener型を静的参照せず、runtime reflectionで存在を確認して接続する。未パッチOS、解決失敗、登録失敗では公開`MediaCodec.OnFrameRenderedListener`へ戻るため、OS側patchはbuild・起動の必須条件ではない。
- private経路ではMediaSync final-output成功、公開経路ではMediaSync input Surface到達という観測範囲を診断上区別する。arm登録時と通知受理時に世代とMediaSync instanceを照合し、arm sequence、Surface、MediaSync errorも確認して遅延通知を拒否する。試験用通知も選択中の経路へ追従させ、永続token集合や独自schedulerは追加しない。
- framework patchは任意の統合資産として保持し、適用する製品だけでtarget buildと実機確認を行う。未適用構成は既存host CIで公開API経路を検証する。

# PR #108 走査時CAS起動の除去と制御直列化の統合

- setup scan、boot EPG sync、background maintenanceからCasController生成とMediaCas/Tuner Descrambler接続を除去し、CA descriptorとfree_CA_mode等の意味情報収集に必要なPMT filterだけを動的更新する経路へ分離した。走査中にECM/EMM filterやTRM CAS資源を確保しない。
- CasControllerの専用executorと同期呼出しを除去し、live受信contextではCAS状態、tune generation、section filter、resource loss、DescramblerをTunerControllerの既存executorへ閉じ込めた。MediaCas Handlerからの通知だけを同executorへ非同期配送する。
- 再生準備状態の参照をMaleicacidLiveSessionからTunerController経由へ統一し、CasControllerへ別の直列化境界を作らないようにした。
- TIS設計のscan ownership、MediaCas/TRM接続条件、CAS close再試行、scan資源喪失処理を実装へ追従させた。新しいworker、CAS資源管理器、待機queue、公開APIは追加していない。
- 検証は既存のTISホストCI、Robolectric、Kotlin品質検査で行う。Android/Soong全体、device atest、VTS、実機のTRM回収と放送受信は未実施。

# PR #108 後続ECMで既存の鍵結合と再生を維持

- `CasController.onEcmSection()`は通常のECM失敗で既存の成功状態を消さず、初回の未結合時だけ`setKeyToken()`を行うよう変更した。後続の正常ECMで再結合せず、既存のPID接続処理を維持した。
- `TunerController`は通常のECM診断だけで再生を停止しない。初回の開始条件と、MediaCas無効化・資源喪失・退役時の既存の停止経路は維持した。
- TIS設計へ初回結合と後続ECMの扱いを正本化した。既存のセッション試験を結合回数1回へ変更した。既存のホスト再生準備試験で初回ECM失敗・後続失敗・再結合の不実行を確認し、初回結合失敗・資源喪失後の利用不可も維持した。試験数は増やしていない。
- 検証は既存のTISホストCIとRobolectricで行う。Android/Soong全体、atest、VTS、実機確認は未実施。

# PR #108 ECM失敗直後の復旧で同じ再生署名を再開

ECM失敗はpipelineを停止するため、SessionもCAS unavailable受理時にStoppedへ遷移する。失敗と復旧が近接し、後続の再評価が復旧後のcurrent linkageを観測した場合でも、停止済みpipelineをStartedの署名一致で省略しない。旧generationの通知拒否は既存入口のまま維持する。TisReviewBoundaryTestで実Sessionへ旧/現世代のCAS unavailableを投入し、現世代だけがStoppedへ変わり同じ署名を再開可能になることを確認する。新しいretry状態・timer・通知APIは追加せず、既存playback lifecycleの停止事実を補完する。追加1試験でhost期待件数を284件とし、クラス数は38/35のままとする。

確認結果: 90c3b5eの全283件を含む5 workflowは成功。本追補の変更Kotlinコンパイル、関連61試験、ktlint/detektと差分検査も成功。全284件はpush後のCIで再確認する。実画面出力・CAS backendとの実復号・Soong/device atest/実機VTSは未実施。

# PR #108 video source identityとCASライブ開始条件の補完

- video trackの実画素寸法を、現在のplayback generationだけでなく、既存AvPlaybackSignatureのservice/PID/stream_type/decoder構成と照合する。同じgeneration中にPMTが別PIDへ変わり、tracks更新が再起動より先に実行されても旧ESの寸法を広告しない。同service内の別video trackにも流用しない。新しいgeneration台帳や推定geometryは追加していない。
- 放送事実から得るclearLivePlaybackStaticallyEligibleは従来の静的判定として維持し、既存ServicePolicyDecisionへcurrent CAS linkageを入力するlivePlaybackEligibleを追加した。CA解決済み・登録適格が前提で、CAS必須serviceは実linkage成立時だけ再生を許す。placeholder一律拒否を除き、metadata更新後と再生開始時に同じ判断を使う。保存済みProvider policyは参照しない。
- CAS利用可否はCasControllerの既存session所有から都度算出する。同世代・current配送bindingの各sessionについて、最新ECM成功、token結合、全必要ES PIDのadd成功を要求する。物理token所有はECM失敗時もVOID/close完了まで残るため、最新ECMの成功状態とは区別する。metadata再投影だけでECM失敗を成功へ戻さない。診断専用token、未結合、複数sessionの一部未成立、退役・資源喪失は利用可能にしない。
- ECM処理で利用可能service集合が変わったときだけ既存の世代付きCAS通知からSession再評価へ進む。通常ECM反復で再起動せず、SI parserへの投入やSI ingest sequence更新もしない。ECM失敗は既存pipelineを停止してCAS unavailableを通知し、PMT/SIとECM再取得の経路を維持する。Tuner開始直前にも既存controller executorでtune generation・service・CAS結合を照合し、既存clear ES/block-model/first-output経路へ進む。

| 反例 | 試験・確認 |
|---|---|
| 同一世代AVC PID A→B、同serviceの複数video、同PID構成変更 | TisReviewBoundaryTest: 実Sessionのtrack投影helperへPMT更新順序を入力し、旧geometryの非広告と新世代実値を確認 |
| 静的clear条件だけでCAS必須serviceを常時拒否 | TisReviewBoundaryTest: current linkageによる許可、CA未解決/登録不適格/placeholderの拒否 |
| ECM成立前・一部session/PIDだけ成立、token拒否 | CasPlaybackReadinessTest: 実Framework adapter→CasController→Descrambler、全session/PIDの成立待ち |
| ECM失敗をmetadata再投影で消去、通常ECMごとの再通知 | CasPlaybackReadinessTest: 失敗保持・復旧時だけ通知・旧世代/別service拒否・VOID清掃 |
| token成立後もpipelineへ到達しない | PlaybackFailureCallbacksTest: 実CAS通知→Tuner再評価→既存pipeline開始まで実行。token前/旧世代/解放後は開始せず、token後は通常pipelineのSURFACE_NOT_SETまで到達する |

追加6試験に合わせ、host CIは283件・検出38/実行35クラスとした。新しい検査抑止は追加していない。ECM失敗時の停止処理が長くなったため、同じTunerController内のECM配送処理だけをprivate helperへ移した。資源owner・公開CAS API・別player・第二の鍵経路は追加していない。

検証結果: Android 15入力によるproduction/全試験Kotlinのhostコンパイル、関連6クラス60試験、変更Kotlinのktlint/detekt、差分検査が成功。host全283件はpush後の実SI JNI付きCIで確認する。Soong、device atest、実機VTS、実TRM回収、実画面への出力、CAS backendとの実復号結合は未実施。TISの開始条件を接続した結果をCAS plugin本体・本番Ks共有の完成やr52全体100%一致、CDD/ARIB全面適合の証拠とは扱わない。

# PR #108 HEVCの旧受入試験期待値の追従

73b32a5のhost全277件を実行した結果、HEVC metadataと再生可否の分離を検証する既存試験にr51のselection拒否期待値が1箇所残っていた。r51限定のmetadata判定とProviderへ再生可否を混入しない検証は維持し、r52のgeneric selectionはHEVCを選ぶ期待値へ訂正した。production変更・試験削除・件数変更はない。初回CIの失敗はこの1件のみで、追加したEPG v1→v2→v3の回帰を含む残り276件は成功。変更試験のhost再コンパイルが成功。単独実行はローカルにRust SI JNI libraryがないため起動できず、実JNIをbuildする全277件のCIで再確認する。

# PR #108 TIS全面監査の9指摘への実装追従

- CAS初期化失敗とMediaCas回収でTuner受信そのものを失効する前回実装を訂正した。CasControllerのCAS世代失効は維持し、既存のCAS利用不能cleanupからECM/EMMと再生だけを停止する。PMT/SIとTuner受信は維持し、Tuner資源喪失通知は実際のTuner回収経路だけから発行する。
- 地上波JPNのraw 1..15を既存ISDB年齢domainへ写像する。16/17の衛星拡張、例外値、未解決profile、未対応countryを区別し、同じMapperをProgramとlive判断で使う。
- caption management未受信時の言語track生成を除き、受信したlanguage_tag 0/1だけを広告する。PESによるmanagement収集は言語trackの公開前から維持し、既存のPES callbackからtracksを更新する。superimposeも未受信時のlanguage ID補完を除いた。
- EPG削除windowの境界を直前完成版と現版だけから作る。同じ版の再投影では同じold/new区間を保持し、過去にunionした区間を次版へ累積させない。
- SetupActivityが使うisOwnInputIdを既存の一意解決結果との比較へ統合し、複数登録時のcandidate一致を受理しない。
- parental query失敗でALLOWEDを保持する操作からvideo availability通知を除いた。許可状態のまま再生開始を進められる一方、available通知は既存first-output callback内の判断だけに残した。
- 現世代のcodec header / decoder outputの画素寸法をvideo track投影へ渡し、既存signature比較で再通知する。decoder cropの全項目・範囲を検査し、不正値、旧世代、別codecの値からgeometryを作らない。
- HEVCのselection、MIME、Tuner AV subtype、有限header probe・startup/steady予算、VPS/SPS/PPS CSDを既存block-model decoder経路へ接続した。共通のdecoder capability照合・MediaSync first-output・unsupported診断を使用する。実装と13試験・実header fixtureはPR #57の659622cから対象部分を照合して採用し、現ブランチのCAS回収後処理を維持した。AVCとHEVCのbit readerは既存処理を共通化し、通常ES/AUのcopyや再構成は追加しない。
- managed MediaCasのECM成功後に標準session IDを既存TunerKeyTokenへ渡し、CasController所有のDescramblerへ結合する。ECM失敗、VOID予約値、空/長過ぎるIDを成功tokenにせず、診断専用接続は引き続きDiagnosticOnlyとする。

検証の対応:

| 反例 / 契約 | 実行する証拠 |
|---|---|
| CAS失敗でSIまで停止、Tuner喪失へ誤分類 | PlaybackFailureCallbacksTest: 初期化失敗/MediaCas回収の双方でPMT/EIT維持、Tuner通知なし、再生停止、解放再試行 |
| 地上波ratingの消失 | AribRatingMapperTest: raw 1..15、衛星拡張との分離、未解決profile |
| 未受信/未対応字幕言語の広告 | TisReviewBoundaryTest: 空、tag 2/7、tag 0/1、management消失、広告前のPES収集 |
| 全履歴へ広がるEPG削除区間 | TisR51FixedPlanAcceptanceTest: v1→v2→v3、同版の重複投影、既存の未完成版/収集世代reset試験 |
| 複数inputIdでもsetup開始 | TisReviewBoundaryTest: 実ResolverへFramework登録0/1/2件と別candidateを入力 |
| parental許可保持だけでavailable | TisReviewBoundaryTest: query失敗後のIdleを保持し通知経路へ進まない |
| video trackのgeometry欠落/推測 | TisReviewBoundaryTest: 未取得、実寸1440x1080、decoder crop、0/負値/範囲外 |
| HEVCを一律拒否、欠落/不正headerで起動 | HevcPlaybackTestと既存selection試験: 実VPS/SPS/PPS、CSD、分割受信、crop、切断・予約値・不正escape |
| ECM未成立/不正IDでtoken結合 | FrameworkCasCloseTest: 実adapter→CasController→Descrambler、ECM失敗、VOID/空/17-byte ID |

追加21試験に合わせ、CI期待件数を277件、検出/実行クラス数を37/34へ更新した。新しいTooManyFunctions抑制は試験クラス2箇所のみ（採用元のHevcPlaybackTestとPlaybackFailureCallbacksTest）。独立したシナリオを同じ対象/fixtureで検証し、関数数だけを理由にfixtureや所有を複製しないためで、各宣言に理由を記載している。production側の抑止追加はない。

確認結果: productionと全試験KotlinのAndroid 15入力によるhostコンパイルが成功。CasControllerSessionTest / CasControllerStateTest / FrameworkCasCloseTest / PlaybackFailureCallbacksTest / TisReviewBoundaryTest / HevcPlaybackTest / AribRatingMapperTestの関連71件が成功し、変更Kotlinのktlint・detektと差分検査も成功した。EPGの追加回帰試験を含むhost全277件はpush時点では未実行であり、Rust JNIを含むCIで確認する。Android/Soong build、device atest、実機VTS、実TRMのpriority回収、実機HEVC再生、CAS backendとの実復号結合は未実施。TISの標準session ID受け渡しを実装したことを、CAS plugin本体・本番鍵共有・r52全体の完成とは扱わない。

# PR #108 TRM対応MediaCas接続と資源回収の実装

- Live/scanの既存受信contextからservice Context・framework session ID（scanはnull）・既存用途priorityを渡す。既存main Looperでlistener付きMediaCasを構築し、TRM不在・登録失敗時に診断用constructorへfallbackしない。製品接続はLIVE/MULTI2のtyped sessionを生成する。
- CasControllerのplugin所有へ初期化状態と受信世代を加え、Frameworkの初回容量反映後callbackから現在のmetadataで再開する。5秒の単調時計期限、非正容量・無通知・構築失敗を失敗へ確定し、重複・旧instance・期限後の通知による復活を防ぐ。構築中の取消しでも元のownerを保持して遅延結果を閉じる。
- MediaCas回収時に配送indexと受信世代を失効し、既存TunerControllerから再生・filterを停止して元の世代へ通知する。Frameworkがclose済みのsessionへVOIDやSession.closeを再実行せず、残存Descrambler、MediaCasの順に解放する。独立cleanupを最後まで試し、失敗所有は既存の再試行経路に残す。
- 初期容量待ち・期限・失効・旧owner・遅延構築・別controller独立性・回収時cleanup再試行を7件、typed API選択を1件、実TunerControllerへの回収通知から受信停止・失敗cleanup再試行を1件追加した。CI期待件数は247から256へ更新し、検出35クラス／実行32クラスの条件を維持する。
- 所有を別台帳へ分散させないためCasControllerだけにLargeClass抑制を追加し、全cleanup失敗を診断しcontrollerを存続させる通知処理だけにTooGenericExceptionCaught抑制を付した。各理由は宣言にも記載する。新しい容量調停器・専用worker・CAS plugin通知APIは追加しない。

確認結果（本変更のTIS接続・寿命管理に限定）:

| 既知NG / 反例 | 処理経路と証拠 | 結果 / 未分類 |
|---|---|---|
| 容量通知前のopen、重複・旧owner・期限後の復活 | `CasControllerSessionTest`の初期容量・期限・取消し・遅延構築試験 | OK / なし |
| 型なしsessionへの誤接続 | `FrameworkCasCloseTest.managedAdapterUsesLiveMulti2TypedSession`でAPIへ渡す実値0/8を検査 | OK / なし |
| 回収済みsessionの再close、失敗所有の消失 | `CasControllerSessionTest`の回収試験、`PlaybackFailureCallbacksTest.mediaCasReclaimStopsReceiveGenerationEvenWhenCleanupFails`で通知→配送失効→再生/filter停止→cleanup再試行を実行 | OK / なし |
| 既存の通常終了・無効化の退行 | 上記3クラスと`CasControllerStateTest`をJUnitCoreで実行 | 48件成功 / なし |
| 型・スタイル・差分不整合 | 本番/全試験Kotlinを`kotlinc -jvm-target 17`でコンパイル、変更Kotlin 10ファイルへ`ktlint`/`detekt --input`、`git diff --check` | 成功 / なし |

入力はKotlin 1.9.22、Android 15 `android-all:15-robolectric-13954326`、JUnit 4.13.2。コンパイル対象・classpathは`.github/workflows/tis-host-ci.yml`のhost構成に従い、JavaのTuner/Filter試験stubはJREのみの環境のためECJ 3.37.0でコンパイルした。実行は`org.junit.runner.JUnitCore`へ表の4クラスを指定した。初回の追加試験で見つかったfixtureのmap型・実行thread・filter再試行回数の不一致を修正し、最終48件の成功を確認した。

本番constructorの型照合とAOSP `android-15.0.0_r1`の構築・容量反映・session回収順序を確認したが、実TRMへの登録・容量反映・priority reclaimの結合確認は未実施。host全256件、Android/Soong build、device atest、VTS、実機試験は未実施。CAS plugin本体、本番Ks共有、実復号は本変更で実装していない。r52全体の実装100%一致、CDD/ARIB全面適合の判定は **No**。

# PR #108 MediaCas無効化後の終了実装

- Framework MediaCas adapterと全Sessionで無効状態を共有し、CAS固有状態例外・引数エラーを区別する。CasControllerは無効化を検出するとplugin/sessionと配送indexを退役させ、Descrambler閉鎖からMediaCas.closeへ進む。未完了cleanupの所有を保持し、成功した終了処理を繰り返さない。
- metadata適用中の無効化はrollback中も含めて失敗として返し、その更新内の再生成を止める。元の操作例外と追加cleanup失敗を診断に保持する。
- ECM/EMM配送での無効化をTunerControllerの全件cleanupへ接続し、PMTを維持してCAS filterと再生を停止する。停止時の世代更新後も、失敗した元の再生世代へ利用不能通知を返す。
- 実Framework adapterへ不可逆な無効化を注入するhost試験を8件、TunerControllerの実配送からfilter・再生停止・通知までを検証するhost試験を2件追加した。各API境界、複数session、旧plugin終了、rollback中無効化、複数cleanup失敗、後続再接続、別controllerの独立性を確認し、CI期待件数を247件へ更新した。
- 本番・全試験KotlinのAndroid 15入力によるhostコンパイル、関連JUnit 41件が成功。追加試験はhost専用であり、Soong/atest対象へFramework stubを混入させない。Android/Soong build、device atest、VTS、実機確認は未実施。r52のTRM・初期容量待ち・実復号接続は本変更の実装対象に含めない。

# PR #108 MediaCas無効化後の終了設計

- Frameworkのinstance無効化と再試行可能な個別close失敗を区別し、検出・配送停止・所有資源の終了をTISへ帰属させた。CAS固有状態例外をMediaCas全体の無効化へ誤分類しない。
- 無効化後は個別Session.close成功を待たず、全Descrambler閉鎖、MediaCas.close、所有除去へ進む。未完了cleanupの再試行と診断を保ち、Framework後処理完了をplugin releaseや鍵失効の確認へ読み替えない。
- 現行診断接続にも適用する設計変更。AOSP Android 15およびLineageOS 22.1のMediaCasを照合し、文書差分を確認した。このコミットでは実装・build・unit test・Soong・atest・VTS・実機確認を変更・実施していない。

# PR #108 初期CAS容量反映後のsession開始

- MediaCas構築完了から直ちにsession生成へ進まず、容量更新後のstatus callbackをcontrollerへ渡して一度だけ再開する順序にした。
- 通知待ちの期限、非正容量・TRM不在の失敗、再選局・終了との競合、旧instance・重複・期限後の通知を既存plugin所有とcleanupへ閉じた。同期waitや独自容量台帳は追加しない。
- AOSP MediaCasのstatus処理、TunerResourceManagerの初回要求前更新契約、同期AIDLとTRM更新処理を照合。設計のみの変更で、r52接続の実装、build、unit test、Soong、atest、VTS、実機確認は未実施。

# PR #108 CAS容量初期通知の受信順序

- r52のMediaCas構築時にlistenerを指定し、初期status eventをTRM登録完了後に処理する順序を明記した。容量値の正本とTRM更新はCAS設計・AOSPに置き、TISへ容量集約処理を追加しない。
- 設計文書のみの変更。現行r51 adapterの実装変更、build、unit test、Soong、atest、VTS、実機確認は未実施。

# PR #108 CA system IDとsession数通知の正本参照

- SupportedCasSystemIdsの数値対応は開発規則、Framework/TRMへのsession数通知と資源枯渇はCAS設計を正本として参照する。TISへ別の容量集約・通知責務を置かない。
- 既存Kotlin定数と正本の対応を確認した。設計文書のみの変更で、production code、build、unit test、Soong、atest、VTS、実機確認は変更・実施していない。

# pr108_cas_review_resource_boundaries

- r52の設計として、TRM対応MediaCasの生成条件と、強制session close後の配送停止・Descrambler閉鎖・plugin退役を定義した。現行r51の診断用接続をr52対応済みとして扱わない。
- plugin共有の範囲を同一CasController内に限定し、別のライブsession・scan contextを横断する共有要件を除いた。
- AOSP Android 15のMediaCas/Tuner資源寿命と設計間の経路を照合。今回の変更は設計文書のみ。build、unit test、Android/Soong、atest、VTS、実機確認は未実施。

# pr108_cas_session_ownership_and_reclaim

- AOSP Tunerの資源回収通知を通常cleanupから分け、FrameworkによるDescrambler閉鎖後はVOIDを再投入せず、bridge閉鎖確認からMediaCas session/plugin解放へ進む。
- CA systemごとに1個のMediaCas pluginを保ち、service・ECM PID・private dataごとにsessionとDescramblerを所有する。旧sessionの解放後に新sessionを生成し、存続sessionとEMM ownerを維持する。
- 通常cleanupのVOID・session close・Descrambler close・plugin closeの順序と再試行を更新。資源回収、同一CA system内のsession分離のhost回帰試験を追加し、CI期待件数を237件、検出クラス数を35件へ更新した。
- Kotlin本番・全試験ソースのhostコンパイルとCAS関連JUnit 23件、変更Kotlinのktlint/detektを確認。host試験全237件、Android/Soong build、device atest、VTS、実機確認は未実施。CAS plugin本体の実装完了を示すものではない。

# r51_tuner_hal2_audit_regressions

- BS事前scanは`onLocked()`で同一scanを一度だけ継続し、`onScanStopped()`まで待機する。状態変更は既存の単一controller executorに限定し、追加の同期ロックを置かない。公開scan契約への接続を設計へ反映した。
- 重複LOCKED、継続開始失敗、終了後の遅延LOCKEDに対するKotlin host試験を3件追加し、CI期待件数を235件へ更新した。Android/Soong、実機earth_pt1/px4/実放送波は未実施。

# r51_pr91_review_truncated_loop_and_identity

- 切断event loopの全受信bytes・宣言長・構造状態と、先行する正常descriptor事実を実JNIから診断専用DTOへ保持する。
- 通常event/除外eventのstableIdentityをnullableにし、キーのないeventをMapperでProgramへ昇格させない。時刻状態ごとのキー有無と切断loopの回帰試験2件を追加し、CI期待件数を188件へ更新。
- 検証結果はPRへ記録する。Android/Soong実体build・実機VTSは未実施。

# r51_pr91_review_retry_and_diagnostic_facts

- 非authoritativeな通常upsert成功による旧dirty削除要求の誤消去を防ぐ。現在のauthoritativeなprovider処理成功でのみ要求を除去する。
- 除外eventの完全な記述子事実を診断専用DTOで通常bulkへ保持し、公開候補へは戻さない。transaction DTOのfield・key型とsnapshot取得契約を現行実装へ揃える。
- 同一区間の非authoritative更新と、最大長のmalformed parental descriptorを実JNI経由で確認する回帰試験2件を追加。CIの期待件数を186件へ更新。検証結果はPRに記録する。Android/Soong実体build・device atest・実機VTSは未実施。

# r51_pr91_review_publication_boundaries

- 構造検査に失敗したsectionのeventをProgram候補から除き、診断は通常snapshotに残す。Mapperへ実際の収集profileを渡し、媒体の暗黙固定を除去する。
- 再試行区間全体を現在のauthoritative区間で確認し、現在のキーを使用する。再試行がある場合はfingerprint一致で省略しない。重複した旧retry設計を正本参照へ統一する。
- 回帰試験4件を追加。検証結果はPRに記録する。Android/Soong実体build、device atest、実機VTSは未実施。

# r51_pr91_review_publication_failure

- EPG公開policyをKotlinへ集約し、dirty retryはServiceKey/時刻区間の再検証要求だけを保持する。現行の完全EITがなければ保留する。ProviderDataResultをSuccess/Failureへ閉じ、失敗serviceの書込み・削除・fingerprint・Direct Boot完了を止める。current EIT ratingはprovider query前に採用する。
- 検証: GitHub ActionsでSI host、TIS host（Kotlin 180件）、HAL hostのunit tests/Clippy/型検査、VTS profile検査が成功。production Rustの整形差分も反映した。Android/Soong全体、AIDLサービス実体試験、実機VTSは未実施。

# r51_pr85_stack_review_followup

- TIF相対音量をsession所有・初期値1.0・0..1 clamp・AudioTrack再生成時再適用として設計正本へ固定した。既存実装のsession/playback executor境界を維持し、global volume ownerは追加しない。
- scan用Tunerのresource-lost callbackをactive tune generationへ接続し、該当candidate以後のSI snapshot/TvProvider publishを拒否してtaskを失敗終了する。boot同期はpendingを維持し、既存schedulerへ再試行を返す。
- current tune generationで受信したlatest EIT ratingを保存済みcurrent Programより優先し、同一eventの再時刻化またはevent切替で旧ratingを継承しない。TvProvider query失敗は情報不存在へ丸めず、上位の既存access-state保持へ返す。
- Programs optional列を`SET/CLEAR/KEEP`へ分け、authoritative snapshotだけが不存在値をclearし、partial snapshotは旧正常値を保持する。insert時のKEEPは未設定/SQL NULLとする。
- playback budgetをdecoder能力値ではなくcodec family別のTIS保持量policyとして固定し、実decoder適合は既存MediaCodec選択・configure・queue・deadlineでfail-closed判定する。実機qualificationはrelease証拠として残し、測定値をruntime profile入力に見せない。
- Kotlin host回帰試験を1件追加し、CI期待値を173件へ更新した。Android/Soong build、device atest、CTS、実機Tuner/VTS/実波確認は未実行。

# r51_pr85_stack_independent_startup_deadline

- ADTS/PCEのstartup構成をRust共通部品へ統合し、二重音声を1chへ推測せず実PCEから設定する。JNI上限超過、PCE待ち、正しいASC byte alignmentをhostで検証する。

- TIS-041/R07に関連するADTS設定を放送ASCへ接続し、HE-AAC拡張周波数・profileと8ch構成を正しく渡す。未解決PCEを1chへ捏造しない。decoder callback設定失敗も生成済みdecoderのrollbackへ含める。

- #87 N06に関連する一時解除の期限を受理時点へ固定。終了時刻変更・時計後退・event_id再使用で延命せず、期限時の実再評価とretune/releaseでのタイマー失効を接続した。

- AVC記述子とSPSの整合を検証し、MediaFormatのprofile/level・寸法・音声構成に対応する実decoderを選択。PMT codec根拠をEIT統合時にも保存し、ALS・未解決音声を再生選択から除外。

- 表示名の正本と長文標準列の全長投影を明記し、公開可否診断を実行中に限定した。

- 走査目的ごとの必要instance集合をTISで評価し、boot/backgroundでは物理候補ごとに固定した全ServiceのEIT完成を待つ。有限走査の全終了経路でsection filterを明示stop/closeし、ライブの継続監視と契約を分離。

- 完成した正常空EITを対象Serviceごとの確認済み空更新として扱い、所有channelとProgramの必須問い合わせ成功後だけboot同期完了へ算入。区間なしの削除は実行しない。

- 走査期限を単調時計へ変更し、SI collection上限の失敗statusを定義。

- EIT instanceの完成・不足section・版状態を通常の登録用／EPG用snapshotで透過的に受け取る。

- #87 D04 / TIS-AUD-05: AV filter開始からcodec別の起動期限を独立に予約し、無入力・少量入力・構成済み無出力でも終了する。audio-onlyは利用不能、audio-videoのaudio失敗はvideo-only新世代へ移る。初回出力とcloseで期限を解除し、flushで期限を延長しない。
- 起動段階、期限境界、初回出力、終了済み世代の回帰試験を追加する。
## r51_pr85_review_20260909

- AV filter開始時にcodecごとの起動期限を予約し、入力が途絶えても期限を判定する。最初の非空出力、終了、世代失効で解除する。既存#91から現行設計への実装追従部分だけを移した。
- EIT公開section条件をTISへ置き、Rust bulkの表完成状態を通常Kotlin snapshotへ渡す。
- Kotlin本番・全試験ソースのコンパイル、起動期限と公開条件のJUnit 5件が成功。CI全件数を156へ更新。実JNIとの試験、Android build、atest、CTS/VTS、実機確認は手元では未実施。

## r51_pr85_cas_session_diagnostics

- TIS-047: MediaCas plugin 自体を生成できない失敗を `PLUGIN_UNAVAILABLE`、plugin 生成後に session を開始できない失敗を `SESSION_OPEN_FAILED` として区別する。B25 の EMM 経路も同じ型付き分類を使用する。
- session 開始に失敗した MediaCas bridge を直ちに close し、どちらの失敗でも key token や elementary PID を descrambler へ接続しない。
- host Kotlin 状態遷移試験で plugin 未接続と session 開始失敗を別々に固定し、CI の成功件数を 152 件へ更新する。実 CAS HAL、Tuner HAL 診断相関、実機 descramble は r51 の完了対象に含めない。

## r51_pr85_playback_stop_admission

- TIS-040: 再生資源が確保されている pipeline を LiveSession 数とは独立して計数し、boot EPG / background scan の事前検査・受付後検査・実行直前検査へ接続した。最後の pipeline の停止確認時に保留ジョブを再評価する。
- 解放に失敗した Filter、decoder、MediaSync、Surface、AudioTrack を再生 owner に保持し、次の停止処理で再試行する。全資源の解放が確認できるまで再生中の計数を下げない。
- LiveSession / TunerController の終了は一件の失敗後も残る資源を解放し、失敗を合成する。終了未確認の LiveSession を件数から除かない。
- 検証: Kotlin 本体・テストコンパイルと JUnit 151 件に成功。CI の発見 class 数を 32、実行 class 数を 29、成功件数を 151 へ更新。以前の CI は 149 件成功後に旧件数 147 のチェックで失敗していた。

## r51_pr85_filter_input_continuity

- TIS-010: RestartEvent を MediaEvent と同じ順序で処理し、同じ再生構成の restart と OVERFLOW 後の通常 flush では未投入 MediaEvent と使用権だけを回収する。decoder、MediaSync、AudioTrack、PTS 基準、再生世代を保持する。
- TIS-021: 字幕 Filter の restart、overflow、短い PES 読取りを字幕連続性喪失へ接続し、native 字幕 facts、字幕 decoder、表示・予約を初期化する。AV の通常 flush はこの初期化を呼ばない。
- 旧 Filter から遅延した MediaEvent も所有権を回収して破棄する。
- 検証: Android 15 の RestartEvent 契約を一次資料で確認。Kotlin 本体・テストのコンパイルと JUnit 149 件に成功。Android Filter の実 callback による統合試験は未実施。

## r51_pr85_caption_viewport_rerender

- TIS-018: 同一再生世代の viewport 変更では字幕 decoder を維持し、libaribcaption の現在時刻での再描画を JNI へ接続した。現在の有効区間に属する画像だけを新しい描画領域へ戻し、古い bitmap を拡大縮小しない。
- 一時的な不正・未確定領域でも復号状態を破棄せず表示を消去する。旧 viewport の予定画像は表示時に再描画し、停止中の時計でも現在時刻に到達した表示・消去を処理する。
- 検証: 字幕 Rust 15 件、全 target Clippy、Kotlin コンパイルと JUnit 149 件に成功。実 libaribcaption の画素出力・実機表示確認は未実施。

## r51_pr85_provider_publication_bytes

- TIS-033: 実際の channel ID と最終 ContentValues、更新区間を準備し、固定順の長さ付き byte 列全体から SHA-256 を計算する。同じ準備済み行を書き込み、成功時だけ処理内キャッシュへ反映する。行ごとの要約連結、仮 channel ID、余分な改行を公開経路から除いた。
- TIS-026: TIS が直接設定した canonical genre と TvProvider 書込み後の読戻し値を独立した診断として保持する。読戻し失敗は診断に残し、成功済みの公開を失敗へ変更しない。
- TIS-036: TvProviderWriter の provider-data 読取りも blob に統一し、文字列からの修復を除いた。
- 検証: Kotlin 本体・テストのコンパイルと実 JNI を使う JUnit 149 件に成功。実機 TvProvider の補完動作確認は未実施。

# r51_pr85_service_semantic_boundary

- SI U-09/U-10/U-11: 通常JNI consumerをservice/transport意味snapshotへ統一する。CAS metadataとPMT索引は同じservice factsから導出し、CATとservice-scoped CAを混同しない。欠落名はnullのまま保持する。
- SI U-13: component保存用codec名をRustから受け取り、Kotlin側のcodec命名表を除去する。既知の製品再生対応stream_type集合は製品policyとして維持する。
- Kotlin本番/試験コンパイルとhost JUnit147件が成功した。Android実機試験は未実施。

# r51_pr85_raw_descriptor_publication

- SI U-03/U-19: Rust生成descriptor事実JSONをEvent/Programモデル経由でprovider-data builderへ透過返却する。Kotlinでcountry byte・descriptor解析状態・unknown rawを組み直さない。
- Kotlin本番/試験コンパイルとhost JUnit145件が成功した。実JNIのEIT入力から保存・再正規化まで、正常ratingだけの昇格と診断raw全体の保持を検証した。Android実機試験は未実施。

# r51_pr85_provider_boundary_corpus

- SI-014: provider-data共通境界fixtureを実JNI経由で検証し、Program正規化・キー抽出・Channel復号の拒否条件をRustとJSON Schemaへ合わせる。既存のasset契約試験もhost CIへ追加する。
- Kotlin本番/試験コンパイルとhost JUnit144件が成功した。Android実機試験は未実施。

# r51_pr85_clock_section_crc

- TIS-AUD-04/F-02: PID 0x14をTDT(table_id=0x70)とTOT(0x73)の完全一致filterへ分割し、TDTだけCRC検証を無効とする。TOTはTunerのCRC検証とRust側検証を両方通す。
- 同じPIDに属するfilter群を一つの所有権で回収し、callbackの発生元を各実filterとtune世代で検査する。準備途中の失敗では作成済みfilterをすべてcloseし、close失敗の所有物と例外を保持する。
- 実Android設定型で256通りのtable IDの非重複選択とCRC設定を確認した。本番/試験Kotlinのコンパイルとhost JUnit140件が成功した。Android実機でのFilter同時開設とCRC不正TOT入力の試験は未実施。

# r51_pr85_discovery_cas_provider_fixes

- TIS-AUD-01/B-06: BS探索が期限内に完了した場合だけ報告済みstream IDを採用し、呼出し側でも成功判定を確認する。TIS-AUD-06/B-11: backend名に依存するselector拒否を除き、公開Tunerの結果で判定する。
- TIS-AUD-02/D-08: background maintenanceの既存channel照会失敗を例外としてscan managerへ返す。空の正常結果へ変換しない。
- TIS-AUD-03/G-05/TIS-047のB1項目: EMM PID収集・登録をB25へ限定し、B1 CATだけでCASを作成しない。B1のECM処理は維持する。
- TIS-008: listener設定が失敗したTunerをcloseし、cleanup失敗があれば元の例外へ保持する。TIS-036: 保存BLOBをgetBlobの結果のままRustへ渡し、文字列へ補修しない。
- TIS-048: 既存channelの更新値からCOLUMN_TYPEを除く。TIS-050: short_event本文の未規定な256文字切詰めを除く。TIS-AUD-07/H-14: READMEの規約参照を実在する共通規約へ修正する。
- CIと同じKotlin 1.9.22・Android 15入力で本番と試験をコンパイルし、実SI JNIを使用するhost JUnit 138件が成功した。既存のコンパイル警告は残る。Android実機のTuner/CAS/Provider統合試験は未実施。

# 未リリース

- helper追加: `MediaSyncFirstOutputBridge`を追加し、platform-privateな`MediaSync.OnFirstVideoFrameQueuedToOutputListener`型とsetterを実行時reflectionで解決して呼び出す。stock platformへの静的hidden API型依存は持たない。
- 公開到達経路: private listener経路が呼び出し可能な場合はExact modeを使用し、API不存在、reflection解決失敗、登録setter呼出し失敗などを含めprivate listener経路を呼び出せない場合は公開APIの`MediaCodec.OnFrameRenderedListener`を型付きで`setOnFrameRenderedListener()`へ登録してCompatibility modeへfallbackする。current codec、playback generation、待機開始時刻、Surface、MediaSync surface errorを確認し、Exact modeとCompatibility modeは診断上区別してCompatibility modeをfinal-output成功と同値には扱わない。
- platform差分: `frameworks_av_mediasync_first_output.patch`と`frameworks_base_mediasync_first_output.patch`の本文は変更せず、Exact modeの正規platform実装として維持する。
- 文書追従: Exact/Compatibilityのruntime契約を`DESIGN_JA.md`、platform patch適用、target build、実機確認を`INTEGRATION.md`へ集約した。`tis/platform_patches/lineage-22.1/README.md`の統合手順は`INTEGRATION.md`へ移し、許可外Markdownを削除した。
- テスト期待値・host境界: host用`MediaSync` stubからprivate listener/setterを削除し、TIS production sourceがstock platform APIだけで静的compileできる境界へ変更した。
- 検証: TIS host CIでKotlin production compile、host tests、caption関連、XML contractを確認する。Android/Soong target build、atest、VTS、platform patch適用後の実機late-drop非通知・final-output one-shot・stale sequence棄却は未実施であり、`INTEGRATION.md`の製品統合gateとして残す。

# r50ef_review_followup_7

- `MediaEvent.getPts()`をproducer確定済みのauthoritative metadataとしてTISがopaqueに受理する責務を明記した。HAL producer側のcodec header構造検証、PES横断bounded residual、first-AU位置、actual sample rate、exact sample countによるassociationを、TIS側で禁止するgenericな0／前値／PCR／wallclock補間と明確に分離した。
- TISはproducer側codec parser／associationを再実行・複製せず、`isPtsPresent`を元PES headerのprovenanceとして維持し、`isPtsPresent=false`でもauthoritative `getPts()`を既存consumer pathへ透過する。producer個別実装の完成をTIS文書から主張せず、consumer責務だけを固定したため、新しいstate owner、parser、queue、clockは追加していない。
- 文書差分のみで、production Kotlin、公開schema、AOSP API、ARIB parser、future_work、`RELEASE_VERSION`は変更していない。`git diff --check`を実施し、Android/Soong build、atest、CTS/VTS、実機確認は未実施。

# r50ef_review_followup_6

- 音声track切替のrestart失敗でも新playback generationをLiveSessionへ反映し、旧`Started` stateを残さず`Failed`へ遷移するようにした。restart前の入力拒否ではcurrent stateを維持する。
- audio-only serviceの旧generation failureをstate、字幕generation、外部unavailable通知の全境界で破棄するようにした。
- PMT側に同じ`component_tag`が無い場合も、有効なEIT component/audio-component descriptor事実をcanonical componentsへ保持するようにした。
- 上記generation/current-stale/ARIB descriptor保持のbehavior testを追加し、Kotlin host test固定件数を127件へ更新した。
- Android/Soong build、atest、CTS、VTS、実機確認は未実施。本commitのKotlin host compile/testはPR checksを正とする。

# r50ef_review_followup_5

- 音声track切替で再生成したplayback generationとfirst-output待機状態をLiveSessionおよび字幕controllerへ伝播し、旧generationをStartedとして保持しないようにした。
- audio-only serviceのfatal audio失敗をcurrent generationのFailed遷移へ接続し、停止済みPipelineをStartedとして残さないようにした。audio-video serviceのvideo-only fallbackは従来どおり映像unavailable通知から分離する。
- 新しい選局要求ではfrontend settings構築前に旧PlaybackPipeline、section filter、CAS状態を破棄し、settings構築失敗時にも旧live stateを残さないようにした。
- audio switchのgeneration遷移、first-output待機、current/stale generationのfatal失敗をbehavior testへ追加した。
- Kotlin host testの固定件数を追加した2件に合わせて125件へ更新した。Android/Soong build、atest、CTS、VTS、実機確認は未実施。本commitのKotlin host compile/testはPR checksを正とする。

# r50ef_review_followup_4

- Rust bulk snapshotのEIT component/audio-component descriptor事実をPMT streamへcomponent_tagで結合し、TvProvider Program provider-dataまで損失なく運ぶ経路を追加した。
- `freeCaMode.text`と`diagnosticCode`をcanonical保存境界から除去し、表示文言と現行releaseの再生可否をTIS process-local派生へ戻した。
- linkage private-data wire fieldを`privateDataPrefixHex`へ統一し、seriesの有効な16-bit MJDと合わせてRust bulk snapshotからprovider-dataまで確認するhost testを追加した。
- Android/Soong build、atest、CTS、VTS、実機確認は未実施。本commitのRust/Kotlin host検証はPR checksを正とする。

# r50ef_review_followup_3

- Program provider-dataは、TvProviderへpublishする時点でKotlinのtyped requestからRust/Serde canonicalizerを呼ぶ責務境界へ戻した。bulk snapshotの`providerDataCanonicalJson`、`AribEvent` / `ProgramRecord`のshadow field、test-only Kotlin保存schema fixtureは削除した。
- この境界ではTISがAndroid列投影とpublish判断、Rustが保存schema・検証・canonical encodeを所有し、同じruntime stateの二重ownerを作らない。
- Android/Soong build、atest、CTS、VTS、実機確認は未実施。本commitのRust/Kotlin host検証はPR checksを正とする。

# r50ef_review_followup_2

- Rust bulk transaction内で生成したProgram provider-data canonical JSONを`AribEvent` / `ProgramRecord`がopaqueに運び、TvProviderへ同じUTF-8 bytesを書き込む形へ変更した。KotlinのProgram `JSONObject` builderとRustへの戻りJNIは削除した。
- AOSP Tuner Resource Managerへ申告するuse caseを`ScanPurpose`から一意に決め、setup scanは`SCAN`、boot EPG syncとbackground maintenanceは`BACKGROUND`、live sessionは既存どおり`LIVE`に固定した。
- Android/Soong build、atest、CTS、VTS、実機確認は未実施。本commitのRust/Kotlin host検証はPR checksを正とする。

# r50ef_review_followup

- service policy、provider-data入力、TvProvider列、Direct Boot pending、scan task、AV playback、字幕presentationの各状態ownerを一つへ整理し、派生値と失効判定だけを投影境界へ残した。
- Program publish失敗queueを`ServiceKey + updateWindow + notBeforeMs`の単一有界LRUへ縮小し、段階backoff、jitter、attempt、retention、ServiceKey別上限を削除した。最終`ContentValues`の単一SHA-256 fingerprintは更新抑止用に維持した。
- 字幕renderer結果をone-shot packed JNI resultへ変更してframe handle registryを削除し、decoder/scheduler/UI失効を単一presentation epochへ統合した。AV開始lifecycleもSession所有のsealed state一つへ統合した。
- Rust bulk transactionのdiscovery stage/table requirementを同一readへ含め、read回数generation、CAS用重複service/metadata、別stage JNI readを削除した。service componentはcanonical streamから投影し、secondary audio languageも保持する。
- Android/Soong build、Rust unit test、atest、CTS、VTS、実機確認は未実施。静的差分・schema fixture一致・構文参照検査のみ実施する。

# r50ee99_review_wording_precision

- AOSP frozen Tuner AIDLで`RELATIVE_STREAM_NUMBER`が合法なselector種別であることを明示し、永続channel tune identityでは採用しないという製品設計理由へ表現を修正した。
- `Channels.COLUMN_INPUT_ID`の責務をTV input ownership一般ではなく、channelとTvInputServiceの関連付けのSSOTとして限定した。
- ARIB STD-B10 5.13-E1 Part 2 Table 6-25に合わせ、`0x01`を`Digital television service`、`0x02`を`Digital audio service`と表記した。
- schemaおよび実装コード変更なし。文言整合と`git diff --check`のみ確認し、Android/Soong build、Rust unit test、atest、CTS、VTS、実機確認は未実施。

# r50ee98_provider_contract_residual_fix

- ARIB `service_type` を Android generic `SERVICE_TYPE_*` へ変換せず、TvProvider `COLUMN_SERVICE_TYPE` には投影方針正本に従ってARIB codingを保持する設計へ修正した。
- `tis/tests/assets/program_provider_data_v1/unsupported_codec_program.json` からrelease/runtime capability値 `r51PlaybackSupported` / `liveViewableClaim` を削除し、Rust側schema正本と同期した。
- 実装コード変更なし。JSON構文確認と静的差分確認のみ実施し、Android/Soong build、Rust unit test、atest、CTS、VTS、実機確認は未実施。

# r50ee97_future_wording_and_wallclock_research_fix

- TIS 設計・統合文書の r51/r52/r53 表現を、開発規則で定義されたリリース計画を暗黙に再定義しない現行 product / 現行仕様 / 非採用範囲の表現へ補正した。
- 字幕、録画・予約、codec、EPG、boot/background maintenance の境界を、r番号ではなく対応宣言条件と設計正本吸収条件で記述する形へ補正した。
- コード実装変更なし。Android/Soong build、Rust unit test、atest、VTS、実機確認は未実施。

# r50ee96_doc_responsibility_scope_final

- `tis/DESIGN_JA.md` の字幕節で `完了条件` としていた表現を `対応宣言条件` へ補正し、DESIGN_JA.md が完了判定正本に見えないよう文書責務を整理。
- `tis/INTEGRATION.md` の字幕統合境界にも同じ `対応宣言条件` 表現を適用。
- コード実装変更なし。Android/Soong build、Rust unit test、atest、CTS、実機確認は未実施。

# r50ee95_doc_responsibility_readfix

- `tis/DESIGN_JA.md` の `provider-data / retry / attribution 境界の完了条件` 見出しを `provider-data / retry / attribution 境界契約` へ修正した。
- TIS 実装コードは変更していない。
- build / unit / atest / CTS / 実機確認は未実行。

# r50ee93-responsibility-boundary-docs

- r51字幕表示に必要な libaribcaption Soong / renderer 統合境界を `tis/DESIGN_JA.md` と `tis/INTEGRATION.md` へ吸収した。
- `future_work/r51/libaribcaption_android_soong_ready_plan(1).md` のうち現行r51境界に必要な内容を TIS 正式文書へ移し、future_work 側の重複計画を削除対象にした。
- provider-data schema / TvProvider投影 / TIS runtime利用の正本境界を `tis/DESIGN_JA.md` に明記した。
- Android/Soong build、instrumentation test、atest、CTS、実機確認は未実行。

# r50dx10

- r50dx9 のフェーズ5完了条件を再確認し、`PlaybackStartGateTest` に残っていた裸の PID 入力を `TsPid` に修正した。
- これにより、型化後のテストコードで残っていた構文上のデグレを解消した。フェーズ6は未実施。
- Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50dx9

- r50dx8 のフェーズ4完了条件を確認し、フェーズ5として字幕 PTS、CAS token、視聴年齢制限の実動作補強を実施した。
- 字幕 PES の PTS あり経路を `CaptionTimestamp.Pts` として維持し、PTS 欠落時だけ `subtitlePtsFallbackSamplesForDiagnostic()` が増えるようにした。
- `TunerKeyToken` の 0バイトおよび17バイト以上を型生成時に拒否する受け入れ試験を追加し、不正長 token を HAL に渡せない境界を固定した。
- TvProvider の現在番組 rating 問い合わせで null cursor または例外が発生した場合を `ProviderQueryFailed` として分離し、保持済みEIT情報が無い場合は直前の遮断/許可状態を維持して、新規の `notifyContentAllowed()` を出さないようにした。
- Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。静的確認のみ実施した。

# r50dx8

- r50dx7 のフェーズ3完了条件を確認し、ライブ視聴経路が `LivePlaybackSnapshot` の単一取得へ統一されていることを維持した。
- フェーズ4として、ライブセッション作成開始時に実行中の boot EPG 同期を中断し、再開待ちとして保持するようにした。
- フェーズ4として、ライブセッション作成開始時に実行中のバックグラウンド保守処理を中断し、ライブ中の新規要求は開始せず診断へ記録する方針を固定した。
- `SETUP_SCAN` はライブセッション作成時にも自動中断しないことを `liveSessionPreemptDecisionForTest()` と受け入れ試験で固定した。
- Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。静的確認のみ実施した。

# r50dx7

- r50dx6 のフェーズ1・2完了条件を再確認し、静的条件を満たしているためフェーズ3を実施した。
- `LivePlaybackSnapshot` を追加し、ライブ視聴中の service / PMT / CAT / CA metadata / 診断情報を同一 native transaction から取得する境界へ統一した。
- `MaleicacidLiveSession.refreshDynamicSiAndCasFilters()` が `serviceRegistrationSnapshot()` と `casDiscoverySnapshot()` を連続取得して合成しないよう修正した。
- Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。静的確認のみ実施した。

# r50dx6

- r50dx5 のフェーズ1・2完了条件を再確認し、`NativeAribSiParser` の `DescriptorDiagnosticScope` 生成で `eventId` named argument が二重指定されていた未達を修正した。
- これにより、型化後の Kotlin コードで残っていた構文上のデグレを解消した。フェーズ3は未実施。
- Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50dx5

- r50dx4 のフェーズ1・2完了条件を再確認し、テスト側に残っていた `ChannelRecord` の裸の周波数値と `NativeAribSiParser.ingestSection()` の裸の PID 入力を修正した。
- `ScanPlanPolicyTest` の周波数比較を `FrequencyHz.value` に追随させ、テストコードでも `FrequencyHz` / `TsPid` 型境界を維持するよう補強した。
- フェーズ3は未実施。Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50dx4

- r50dx3 のフェーズ1・2完了条件を再確認し、Program 安定キーの Kotlin 手組みが残っていたため、`ProviderDataBridge.buildProgramKey(ServiceKey, eventId)` 経由へ統一した。
- `AribTransport` / `AribRelatedItem` / `AribLinkage` / `DescriptorDiagnosticScope` / `TransportKey` / malformed CA descriptor summary の ONID / TSID / SID を 16 bit 型または `ServiceKey` へ寄せ、内部モデルで裸の識別子を保持しないよう補強した。
- 字幕 PES の時刻引き渡し境界を `CaptionTimestamp` / `PesPts90k` / `CaptionPtsMillis` へ寄せ、TIS 内部で裸の字幕 PTS `Long` を受け渡さないよう補強した。
- フェーズ3は未実施。Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50dx3

- r50dx2 のフェーズ2完了条件を再確認し、型化後のテストコードに残っていた裸の PID / 周波数値 / 不正 ServiceKey 生成を修正した。
- `ChannelRecord` / `ScanCandidate` のテスト入力は `FrequencyHz` を使い、`AribService` / `AribComponentEntry` / `AvPlaybackSignature` / `TisTrack` / CAS metadata のテスト入力は `TsPid` を使うように揃えた。
- `ServiceKey` の 16 bit 値域制限に合わせ、不正 service key は `ChannelRecord` 生成後の検証ではなく `ServiceKey.fromOrNull()` の失敗として確認する形に変更した。
- フェーズ3は未実施。Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50dx2

- r50dx のフェーズ2未達を修正し、section PID の内部境界を `TsPid` に統一した。
- `TunerController` の section filter、動的 PMT/ECM/EMM filter、`SectionIngestController`、`AribSiEngine`、`NativeAribSiParser` の公開境界を `TsPid` 化し、Android/JNI 境界だけ `Int` に戻す形へ固定した。
- `ChannelRecord`、`ScanCandidate`、`TuneRequest`、`ResolvedChannel`、`ProviderDataBridge.ChannelTuneKey` の周波数を `FrequencyHz` 化し、Android/TvProvider/JNI 境界だけ `Long` に戻す形へ固定した。
- `CasController` の ECM/EMM PID index と診断 PID を `TsPid` 化し、不正 PID を内部状態として表現できないようにした。
- Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。Android非依存の型基盤は `kotlinc` で構文確認し、CAS 制御は最小 Android stub 付きで `kotlinc` 構文確認した。

# r50dx

- provider-data JNI result の `success=false` と `bytes` 欠落を失敗として扱い、TvProvider への追加・更新・削除に進まないようにした。
- Program provider-data の旧 `canonicalGenres` request 出力を削除し、Android canonical genre は TvProvider 標準列への投影専用に残した。
- `TsPid`、`TunerKeyToken`、`NetworkId16` / `TransportStreamId16` / `ServiceId16`、`StreamSelector`、`FrequencyHz`、字幕 PTS 型を追加し、CAS / SI / 再生開始署名の PID と token 境界を型で制限した。
- Android/Soong build、instrumentationテスト、atest、CTS、実機確認は未実施。型化対象の一部 Kotlin ファイルはローカル stubs 付き `kotlinc` で構文確認した。

# r50dc

- r50db の provider-data raw bytes 境界修正に対し、TIS instrumentation test 側の provider-data 検査を追随させた。
- Program provider-data bytes は `parseProgramKey(providerData)` へ直接渡し、JSON 内容確認だけを test helper の UTF-8 text view で行うようにした。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50db

- provider-data の既存データ入力境界について、`rawBytes` の意味、invalid UTF-8 / malformed JSON の扱い、署名対象を DESIGN_JA.md に補足固定した。
- `normalizeProgramProviderData` / `programProviderDataSignature` / `extractProgramKey` / `appendCurrentProgramDiagnostics` の Kotlin/JNI 境界を `String` ではなく `ByteArray` 受けへ統一した。
- TvProvider から読み出した `COLUMN_INTERNAL_PROVIDER_DATA` は BLOB bytes を優先して Rust へ渡し、文字列で返る場合も UTF-8 bytes への互換変換だけに限定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50da

- CAS discovery snapshot に malformed CA_descriptor 詳細診断を追加し、Program provider-data の `malformedCaDescriptorCount` は CA_descriptor 診断 summary だけを参照するようにした。
- EventModelMapper が EIT descriptor 診断件数を malformed CA_descriptor count として誤用しないよう修正した。
- audio component の unsupported codec 診断 field を Rust bulk snapshot から TIS component model へ渡せるようにした。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50cz

- r50cy の malformed CA_descriptor 診断粒度固定を前提に、ProviderDataResult を `bytes` / `signature` / `schemaVersion` / `truncated` / `diagnosticsDroppedCount` の設計形状へ寄せた。
- Program publish / CAS discovery の `descriptorDiagnostics` を `DescriptorDiagnosticV1` schema 準拠 DTO として扱うようにし、`AribEventDiagnostic` 要約 DTO への置換をやめた。
- unsupported codec の `r51PlaybackSupported` / `liveViewableClaim` / `diagnosticCode` を provider-data component に保持する経路へ寄せた。
- malformed CA_descriptor count は Program provider-data の診断 summary として保存し、raw descriptor や table/PID/service context を Program ごとに重複保存しない方針に合わせた。

# r50cy

- malformed CA_descriptor 診断の保存粒度を設計に補足固定した。詳細診断は CAS 検出 snapshot または service / channel provider-data 診断を一次保存先とし、Program provider-data には公開時点 summary として `malformedCaDescriptorCount` だけを保存し、raw descriptor や table/PID/service context を Program ごとに重複展開しない方針にした。

# r50cx
- r50cw 静的再確認で残っていた設計・実装不一致のうち、CAS HAL stub と libaribcaption.so 供給方式を除く項目を修正した。
- transaction DTO を `ProgramPublishSnapshot` / `ServiceRegistrationSnapshot` / `CasDiscoverySnapshot` の設計形状へ寄せ、`snapshotGeneration`、`ingestSequence`、publishability map、診断情報を同一 native transaction から扱うようにした。
- `takeProgramPublishSnapshot()` の boolean 分岐を本番 API から削除し、updateWindows を消費しない視聴中参照は `programStateSnapshot()` に分離した。
- 廃止 bulk snapshot wrapper と event diagnostics wrapper を公開通常境界から外し、Native parser の bulk JSON 取得は private 実装詳細へ閉じた。
- SetupActivity の利用者向け英語文言と ScanPurpose enum 名露出を日本語表示へ置換した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50cw
- r51リリース前の設計・実装不一致のうち、MediaEvent sample 上限、transaction DTO境界、provider-data未知キー保持、切り詰め診断、extended_event空項目名、UI文言、README旧情報を設計に合わせて修正した。
- `AribSiEngine` の本番境界を `ProgramPublishSnapshot` / `ServiceRegistrationSnapshot` / `CasDiscoverySnapshot` へ分離し、サービス登録・CAS検出で program publish snapshot を流用しないようにした。
- No.6 の Channel provider-data unknown key schema 変更、および libaribcaption.so 供給方式の固定は今回対象外とした。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50cv
- r50cu 推奨案A固定後の未達1〜8に対し、実装側を設計へ合わせた。
- ProviderDataBridge が JNI へ渡す JSON を `programRequest` / `channelRequest` の受け渡し用形式へ分離し、保存用 schema 名を名乗らないようにした。
- DescriptorDiagnosticV1 の Kotlin typed DTO / 再構築経路を削除し、canonical JSON 文字列の不透明保持へ戻した。
- Program stable key は Kotlin 生成をやめ、Rust JNI 経由で取得するようにした。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cu
- 推奨案Aに従い、TIS と Rust provider-data builder の受け渡し境界を DESIGN_JA.md に固定した。
- TIS は保存用 provider-data JSON を直接生成せず、JNI へ渡す JSON は Rust serde 型への受け渡し用形式に限ることを明記した。
- 実装修正は行わず、固定後の設計と実装の不一致は別レポートで抽出した。

# r50ct
- r50cs 設計・実装不一致レポートのうち、境界整理対象の TIS provider-data input JSON 手組み問題を除き、デグレ1件と未達6件を設計に合わせて修正した。
- DescriptorDiagnosticV1 の canonical JSON は TIS 側で再符号化せず、opaque string として Rust provider-data builder へ返す形にした。
- test source の旧 flat DTO 断片を現行 structured DTO 起点へ寄せた。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cs
- r50cr 設計・実装不一致レポートの10件を、未達とデグレを区別した上で全件修正した。
- Rust bulk event DTO の nested 化に合わせ、TIS parser は `programKey` / `serviceKey` / `timing` だけを通常 event 境界として読むようにした。
- DescriptorDiagnosticV1 は Rust 生成 array JSON を `descriptorDiagnosticsCanonicalJson` として透過保持し、provider-data へ array 単位で保存するようにした。
- `freeCaMode`、`series`、`audioLanguages`、`video` の provider-data 生成で旧 flat field fallback を削除した。
- semicolon 形式の program identity を通常 source と test fixture から削除した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cr
- r50cq 設計・実装不一致レポートの残件1〜7に対応した。
- content genre は Rust の構造化 DTO から TIS が canonical genre / broadcast genre 表示へ投影する形にし、正規表現で `ARIB(...)` 文字列を復元する経路を削除した。
- DescriptorDiagnosticV1 は Rust 生成 canonical JSON を透過保持し、TIS 側で schema / scope / descriptor を field-by-field 再構築しないようにした。
- provider-data video 要約は選択 video component 由来に変更し、parental rating DTO 名を `ratingValue` / `rawRatingByte` へ寄せた。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cq
- r50cp 設計・実装不一致レポートの残件1〜8に対応した。
- Program provider-data へ DescriptorDiagnosticV1、EIT source、選択 audio component 要約を渡すようにし、content rating 逆生成 fallback を削除した。
- Channel provider-data の inputId / displayName を JSON v1 の必須保存値として扱うようにした。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cp
- r50co 設計・実装不一致レポートの残件1〜7に対応した。
- TIS 設計文書から r50 以前 provider-data 互換入力許容を削除し、Channel provider-data の tune 形を JSON v1 の `deliverySystem` / `streamId` / `streamIdType` へ統一した。
- 通常サービス解析経路の component metadata fallback と、Kotlin 側の DescriptorDiagnosticV1 JSON 再構築経路を削除した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50co
- r50cn 後の設計固定に従い、r50 以前の provider-data 互換入力経路を廃止した。
- Channel provider-data 復元は Rust から返る JSON v1 を読む形に変更し、`;` 区切り key-value の fallback parser を削除した。
- Program provider-data builder は `programKey` object を渡す形に変更し、旧 key 文字列入力を通常経路から外した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cn
- r50cm 設計・実装不一致レポートの残件1〜3に対応した。
- EIT component_descriptor / audio_component_descriptor 由来の構造を provider-data components へ渡し、service components と component_tag で統合するようにした。
- DescriptorDiagnosticV1 の Kotlin rawJson 再投入経路を削除し、型付き DTO から JSON を再構成する境界へ変更した。
- EIT section の version / sectionNumber を DescriptorDiagnosticV1 scope へ保持するようにした。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

# r50cm
- r50cl 設計・実装不一致レポートの残件1〜8に対応した。
- provider-data へ TIS 決定の canonical genre を保存し、DescriptorDiagnosticV1 は Rust 由来 rawJson をそのまま渡す境界に変更した。
- `AribEventDiagnostic` の旧 `diagnosticDescriptorJson` を削除し、TIS テスト source を現行 nested DTO へ追随させた。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cl
- r50ck の残件1〜5を静的再確認したうえで、残件6〜9に対応した。
- Kotlin 側の provider-data 境界から raw JSON 断片保持を外し、relatedItems / linkage / components / descriptorDiagnostics を型付き DTO として扱うようにした。
- DescriptorDiagnosticV1 は Rust 側配列を直接受け取り、Kotlin 側で旧診断コンテナから抽出する経路を削除した。
- TIS 側 provider-data asset から旧 `canonicalGenres` を削除し、Rust 側 testdata と byte 単位で一致させた。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50ck
- r50cj 設計・実装不一致レポートの残件5に対応し、TIS の通常 DTO から旧表示用 flat field を削除し、free_CA_mode と series は nested JSON 構造を provider-data へ渡す境界にした。
- Program provider-data へ渡す audio / video metadata に schema required の codec を付与した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50ci
- r50ch の優先順位 1〜4 を再確認し、優先順位 5〜7 として components 構造、未対応視聴年齢制限保持、旧 indexed path 掃除を進めた。
- `AribService` に Rust bulk JSON 由来の `componentsJson` を保持し、event へ attach する経路を Kotlin 再構築優先から Rust 出力優先へ変更した。
- `ratings[]` / `diagnostics.publishDiagnostics[]` に未対応視聴年齢制限を保持する既存経路を維持し、descriptor diagnostic へ戻さない境界を確認した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cg
- `NativeAribSiParser` の未使用 indexed path と private external 宣言を削除し、bulk snapshot のみを通常境界にした。
- Rust bulk event JSON の `descriptors` 構造から番組補足情報を読むようにし、旧 flat event field へ依存しない実装へ変更した。
- Program provider-data へ旧 `canonicalGenres` / `freeCaText` / `seriesName` 入力を渡さないようにした。
- Channel provider-data 生成要求に表示名を渡し、Rust側の nested ChannelProviderDataV1 形と整合させた。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cf
- Rust由来の旧 `canonicalGenres` event field をTISモデルから削除し、canonical genre は TIS の明示写像だけで決定する形へ統一した。
- Kotlin側で未対応視聴年齢制限を descriptor diagnostic JSON として手作りする経路と、`unsupportedDescriptorJson` 旧フィールドを削除した。
- Program provider-data へ渡す未対応視聴年齢制限は `parentalRatingDiagnostics` から Rust側の `ratings[]` / `publishDiagnostics[]` へ統合する経路に寄せた。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50ce
- provider-data / 診断情報の r51 設計固定として、`diagnostics.currentProgram`、ChannelProviderDataV1、未対応視聴年齢制限の格納先を明記した。
- 旧 `canonicalGenres` event field と `nativeGetEventCount()` / `nativeGetEvent*` indexed JNI getter 群を使わない境界を固定し、Kotlin 側の未使用 indexed path と private external 宣言を削除した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cc
- WP-14静的対応として、TIS product統合の正式ファイル `tis/config/product_integration.mk` を追加し、TIS本体、priv-app権限、ARIB SI JNI、ARIB字幕JNI、ライブ TV feature XML を一括で製品へ組み込む手順に統一した。
- `tis/config/product_integration.tis.example.mk` は正式ファイルを継承するだけの例に変更し、`tis/INTEGRATION.md` も字幕JNIを含む製品統合手順へ更新した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50cb
- WP-13対応として、unsupported codec provider-data テストデータを TIS test asset に追加し、Rust側 テストデータと同一内容で保持するようにした。
- `ProviderDataAssetsR51ContractTest` を追加し、provider-data v1 asset、descriptor 診断 element asset、unsupported codecメタデータ が r51再生可能表明にならないことを固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50ca
- WP-12対応として、HEVC `stream_type=0x24` と MPEG-4 AAC LATM `stream_type=0x11` を provider-data component メタデータ として認識しつつ、r51再生可能表明から分離した。
- 未対応 codecメタデータ には `r51PlaybackSupported=false`、`liveViewableClaim=false`、`diagnosticCode=UNSUPPORTED_R51_CODEC`、`parseStatus=UNSUPPORTED_R51` を残し、TIS の video/audio track 選択と再生開始対象には含めないことを test source で固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50bz
- WP-11対応として、r51では録画・予約を表明しない境界をTIS設計と統合手順に明記した。
- `onCreateRecordingSession()` が null を返す r51境界を instrumentationテスト で固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50by
- WP-10対応として、setup / boot EPG sync / user unlock drain が固定文字列ではなく `TvInputManager` から解決した自TISの `TvInputInfo.id` を使うよう修正した。
- setup activity 入口は サービス 側の `BIND_TV_INPUT` 契約とは分離し、システムTVアプリ から起動できるよう activity 側の `BIND_TV_INPUT` 要求を外した。scan 開始可否は inputId が自TISに属するかで判定する。
- inputId 解決不能時は boot EPG sync を pending のまま延期し、不正 inputId で scan / TvProvider 書き込みへ進まない境界をテストで固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50bx
- WP-09対応として、`deletionAuthoritative` が true の更新区間だけ obsolete Program delete に進み、false の区間では既存 Program を削除しない期待へ TIS test を更新した。
- Rust parser から出る `deletionAuthoritative` を `NativeAribSiParser` が受け取り、`ProgramPublishCoordinator` / `TvProviderWriter` の authoritative delete 境界へ渡す経路を確認した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50bw
- WP-08 の CAS 仮実装 境界確認として、診断-only ECM が `setKeyToken()` / `addPid()` に進まないテストを追加した。
- CAS plugin unavailable 時に TIS が fake / 仮トークン を成功扱いしないことをテストで固定した。

## r50bu

- WP-06対応として、r51対象の映像ESがない サービス または r51未対応映像codecのみの サービスを ライブ playback 開始前に視聴不可として扱い、TUNING状態のまま成功扱いにしないよう修正した。
- Tuner が利用不能または `currentTune` 未設定で `PlaybackPipeline.start()` に進めない場合、`notifyVideoUnavailable()` を返す経路を追加した。
- audio-only サービス / HEVC-only サービスが r51非スクランブル視聴成功扱いにならないことを TIS 受け入れテスト に追加した。

## r50bt
- WP-05 未達補修として、`NativeAribCaptionRenderer_nativeDecodePes` に混入していた重複 `let text = match ...` を除去し、字幕 JNI Rust module が構文上成立する形へ戻した。
- `onSelectTrack(TYPE_SUBTITLE, null)` が早期 return で拒否され、字幕track解除分岐に到達しない問題を修正した。
- PTS未指定 sentinel は `Long.MIN_VALUE` / `i64::MIN` のまま維持し、caption disabled 時およびsubtitle track未選択時に描画しない境界を再確認した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bs
- WP-05 未達補修として、TIS 字幕 Rust JNI 境界の `libaribcaption` C API 定数名と値を公開 header に合わせて整理した。
- `ARIBCC_PTS_NOPTS` を `INT64_MIN` として扱い、PTS 未指定の字幕PESを `-1` ではなく libaribcaption の公開値で渡すよう修正した。
- `libmaleicacid_arib_caption_jni_test` を追加し、字幕 JNI 境界の定数値と PTS 未指定 sentinel を test crate で固定した。
- Android/Soong build、Rust 単体テスト、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50br
- WP-05 対応として、PMT由来のARIB字幕ESを `TvTrackInfo.TYPE_SUBTITLE` として通知する経路を追加した。
- `onSetCaptionEnabled()`、subtitle track 選択、ARIB字幕PES filter、Rust JNI字幕境界、`libaribcaption` C API 呼び出し、overlay描画を接続した。
- 字幕trackメタデータを Program provider-data `components.subtitle[]` へ保存する入力経路を追加し、字幕本文・DRCS・BML状態は保存しない境界を維持した。
- caption disabled 時は字幕描画を消去し、字幕PESを自前ARIB SI/EPG文字列decoderへ渡さない制御とテスト観点を追加した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bp
- WP-03 対応として、TvProvider Program 標準列へ canonical genre、scrambled、series id、episode display number、item count を投影する実装へ更新した。
- イベントグループを LONG_DESCRIPTION へ出す経路を除去し、provider-data JSON `relatedItems` 保存へ寄せた。
- free_CA_mode、canonical genre、series / episode / item count、視聴年齢制限、audio language、broadcast genre の provider-data 保存入力を `ProgramProviderDataV1` 前提に更新した。
- 旧仕様の「canonical genre を書かない」期待を持つテストを r51 SSOT に合わせて更新した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bo
- WP-02 対応として、TIS 側の Program provider-data 期待値を `ProgramProviderDataV1` 出力に合わせて更新した。
- 新規 provider-data に旧 `programKeyB64` / `eventGroupText` / `freeCaText` / `seriesName` / `videoFormat` / `unsupportedDescriptorDiagnostics` が出ないことをテスト期待に反映した。
- Kotlin は provider-data 保存 JSON を直接構築せず、Rust JNI の build / 正規化 / extract API を経由する境界を維持した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bn
- WP-01 対応として、TIS 設計文書から Program provider-data schema と descriptor 診断情報 schema の本文再定義を除去し、`arib_si_engine_rs` 側の SSOT と検証用JSONを参照する記述に統一した。
- TIS 側 テストデータ が Rust 側 テストデータと バイト単位で同一であり、schema 検証を通ることを確認した。
- TIS instrumentationテスト の 記述子診断情報 確認を旧 top-level 形の期待から、asset 上の `DescriptorDiagnosticV1` と `ProgramProviderDataV1.diagnostics.descriptorDiagnostics[]` の確認へ更新した。
- unsupported 視聴年齢制限 由来の 記述子診断情報 生成を `DescriptorDiagnosticV1` の element schema に合わせ、`remainingLength` / `rawPrefix` / `serviceKey` 直置きの旧形を出さないようにした。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bm6
- r50bm5 の確認サマリ不足を受け、リリース物規則違反の人手観点を追加確認した。
- 本モジュールの実装ロジックは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bm5
- r50bm4 の確認漏れ是正として、TIS の英語UI文、Deprecated文、テスト診断文、コメントを日本語化した。
- 実装ロジックは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

## r50bm4
- r50bm3 の仕様固定内容を再確認し、イベントグループを LONG_DESCRIPTION へ出す旧記述が残っていた箇所を provider-data JSON `relatedItems` 保存方針へ統一した。
- free_CA_mode の自然対応を `Programs.COLUMN_SCRAMBLED` と provider-data JSON へ反映する方針として明確化した。
- 実装コードは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50bm3
- 承認済みスコープ拡大の仕様固定として、canonical genre 明示写像、series id / episode 標準列投影、イベントグループ provider-data 保存、free_CA_mode / audio language / 視聴年齢制限 投影、字幕 track / libaribcaption 表示経路、r52 codec 固定表を TIS 設計に反映した。
- 実装コードは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認は未実施。

## r50bm2
- リリース物規則違反の追加是正として、TIS 設計文書に残っていた英語自然文、境界見出し、逆圧説明を日本語の現行仕様文へ置換した。
- 仕様 scope と実装 logic は変更せず、文書・コメント・表現整理のみに限定した。

## r50bm
- リリース物規則違反の是正として、CHANGELOG の重複見出し・途中表題・降順崩れを整理した。
- CHANGELOG 以外に残っていた旧版名、作業番号、修正経緯、英語自然文コメントを現行仕様の日本語表現へ置換した。
- 仕様 scope と実装 logic は変更せず、文書・コメント・履歴整理のみに限定した。

## r50bk12
- 仕掛かり修正とは別に残っていた r51 設計契約の未達として、`Programs.COLUMN_CANONICAL_GENRE` を `ContentValues` に直接設定していた経路を削除した。
- boot EPG sync / background maintenance の開始判定に ライブセッション creation in progress を追加し、`TvInputService.onCreateSession()` 入口から `MaleicacidLiveSession` が active session 登録を終えるまでの tuner 資源 race を塞いだ。
- `TisR51FixedPlanAcceptanceTest` に ライブセッション 作成中は boot/background task を開始しないことを固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk11
- r50bk10 後に残った仕掛かり未達として、retry queue の サービス/global cap が DESIGN_JA.md の 32 / 512 と実装の 16 / 128 で不一致だった点を修正した。
- retry retention を enqueue 時と drain/test accessor 時の両方で適用し、期限切れ 再試行区間 を保持しないようにした。
- `ProgramPublishCoordinator` の retry backoff helper に残っていた重複式を除去し、completion test に per-サービス cap / global cap 定数 / retention drop の固定を追加した。
- Android/Soong build、Kotlin compile、instrumentationテスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk10
- r50bk8 completion 版で残っていた完了条件未達を仕掛かり範囲に限定して再固定した。
- SetupActivity の generation / purpose / published 件数判定を pure helper 化し、stale setup Completed、boot/background Completed、invalid inputId では成功終了しないことを test で固定した。
- Program publish の 必須問い合わせ failure、insert failure、廃止行削除 failure、retry failureClass、backoff / attempts / retention を テスト可能な境界 として固定した。
- publish 失敗時に 署名キャッシュ を commit せず、同一入力の retry が unchanged skip されないことを test で固定した。
- Android/Soong build、Kotlin compile、instrumentationテスト 実行、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk8-rerelease
- r50bk8 TIS / arib_si_engine_rs 追加修正計画の7セクションに対応した。
- provider-data 新規 write を Rust/JNI bridge 経由の JSON v1 へ寄せ、Channel/Program の key 抽出・署名 生成・current-program overlap 診断情報 追記を native 側 API で扱う経路を追加した。
- EIT 更新区間 の `deletionAuthoritative` を TIS publish transaction へ伝播し、authoritative でない window では obsolete Program delete を実行しないようにした。
- Program publish retry に failureClass / attempt / nextAttemptAtMs / backoff / 上限 trim を追加した。ただし pending retry は引き続き process-local であり、永続化 store への移行は未実施。
- 本番経路 の snapshot 利用を `snapshotTransaction()` に寄せ、旧 snapshot API を deprecated として明示した。
- SetupActivity を `BIND_TV_INPUT` permission で保護し、自 TIS inputId 検証と scan generation 照合により外部起動・過去 Completed state による成功終了を抑止した。
- CAS descrambler PID type を AOSP `Descrambler.PID_TYPE_T` に修正した。
- AudioTrack 生成時に Android 14 の AttributionSource を可能な範囲で伝播する処理を追加した。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk8
- r50bk7 のフェーズ1〜6静的再監査で、unsupported 視聴年齢制限 由来の `unsupportedDescriptorJson` を `EventModelMapper` が旧 `diagnosticCode` / `descriptorOffset` 形で新規生成しており、フェーズ6の「新規 provider-data write は schemaVersion=1 の canonical 記述子診断情報 shape だけにする」完了条件に未達であることを確認した。
- `unsupportedDescriptorJson` の生成を `schemaVersion=1` / `diagnostics[]` / `parseStatus=UnsupportedValue` / `tag=0x55` / `serviceKey` / `eventId` を持つ canonical shape に変更し、`TvProviderWriter` の migration-read 経路に依存しない新規 write へ固定した。
- r50bk8 は改訂2版 Markdown のフェーズ1〜6完了版として扱う。Android/Soong build、Kotlin compile、Rust 単体テスト、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk5
- r50bk4 のフェーズ3・4A・4B・4C静的再監査で、PlaybackPipeline の MediaCodec first-frame コールバック が main handler から playbackGeneration / surface / videoAvailableNotified を直接参照・更新し得る点を4C未達として確認した。
- first-frame コールバック は playback executor へ enqueue し、playbackGeneration / surface / first-frame state の確認と onVideoAvailable 通知を playback executor 上で実行するよう修正した。
- フェーズ5・6には進まず、r50bk5 は改訂2版 Markdown のフェーズ1〜4C完了版として扱う。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk4
- r50bk3 のフェーズ3・4A・4B・4C静的再監査で、SetupActivity が user-unlock receiver 経由で boot EPG sync を開始し得る点と、ChannelScanController の SI collection 判定が ServiceListBuilder 経由で services / publishability を別snapshot から合成し得る点を未達として確認した。
- SetupActivity から user-unlock drain receiver 登録を削除し、setup activity 起動中は Direct Boot pending を表示するだけで boot EPG sync を開始しないよう固定した。
- ChannelScanController の 登録可能判定を、同一 snapshotTransaction 由来の serviceCounts に一本化し、serviceListBuilder.registrationReadySnapshot() の別snapshot 合成を 本番経路から除去した。
- フェーズ5・6には進まず、r50bk4 は改訂2版 Markdown のフェーズ1〜4C完了版として扱う。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk2
- r50bk のフェーズ1・2静的再監査で、同一 event の start/end が 更新区間 外へ移動した場合に旧 Program row を stable key で発見できず duplicate insert し得る未達を確認した。
- Program upsert の既存 Program index を window 限定から サービス/channel 全体の stable programKey index へ変更し、ONID/TSID/SID/event identity が同じ Program は start/end 変更後も既存 row update になるよう修正した。
- フェーズ3以降には進まず、r50bk2 は改訂2版 Markdown のフェーズ1・2完了版として扱う。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bk
- 改訂2版 Markdown のフェーズ1・2に従い、Program identity / 現在番組 / provider-data 署名 と TvProvider query failure / null cursor / Program upsert safety の実装を修正した。
- `programKey` を ONID/TSID/SID/event の安定キーに固定し、start/end と row id dependent 診断情報を 署名 対象から外した。
- TvProvider null cursor を failure として扱い、`existingChannels()` の空 代替処理 を 本番経路から除去し、サービス 単位 failure 時の 廃止行削除 を禁止した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj3
- r50bj2 後に残っていた設計未固定事項として、malformed descriptor / malformed SI の fail-閉鎖済み 方針、malformed EIT event を 廃止行削除根拠にしない条件、SectionEvent / MediaEvent 入力上限、Direct Boot drain と ライブセッション 優先、TvProvider 必須問い合わせ の null cursor failure 扱いを固定した。
- `ARIB_SI_EPG_TvProvider投影方針.md` に、malformed descriptor / malformed EIT event 由来値を TvProvider 標準列へ正常投影しないことを明記した。
- 実装コードは変更していない。Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj2
- r50bj の設計固定後に残っていた旧未固定記述を整理し、`ARIB_SI_EPG_TvProvider投影方針.md` の `internal_provider_data` schema/key/サイズ上限/LONG_DESCRIPTION 最大長を未固定扱いしないようにした。
- `onUnblockContent()` の start/end は stable identity ではなく current Program row 照合用の補助条件であることを `DESIGN_JA.md` / `INTEGRATION.md` に明記した。
- 実装コードは変更していない。Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bj
- 設計文書上で provider-data JSON v1、Rust serde SSOT、descriptor 診断情報 schema、transaction DTO、session/playback/scan executor 境界、SetupActivity 保護、retry/backoff を固定した。
- `programKey` を ONID/TSID/SID/event_id のみに固定し、start/end を stable identity から外した。
- 実装コードは変更していない。Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi6
- Phase A の lifecycle / generation / executor / cancel / CAS state 未達を是正した。`TunerController` の section コールバック を controller serial executor + generation + filter トークン で隔離し、`onTune()` 解決失敗時も旧 ライブ state を先に破棄するようにした。
- `CasController` の mutation / ECM / EMM / close を専用 serial executor に閉じ、`ChannelScanManager.cancel()` が executor 外から controller / engine を close しないようにした。
- Phase B の別 transport サービス登録を PAT/PMT 由来の actual transport key に限定し、SDT-other / NIT-other / BAT だけで見えた サービスを現在 candidate の物理情報へ紐づけないようにした。
- `NativeAribSiParser` に 本番経路 bulk snapshot ラッパー を追加し、`AribSiEngine` の 公開snapshot path を bulk ラッパー 経由にした。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi4
- 「追加だけで対応できる」テスト補強として、malformed / truncated 相当の 視聴年齢制限 が `Programs.COLUMN_CONTENT_RATING` に投影されず internal provider data 診断へ残ることを 受け入れテスト で固定した。
- unsupported country / out-of-range レーティングが Programs のコンテンツレーティング column に出ず、診断 provider data に残ることを追加確認した。
- 旧 custom `ARIB_JP` / `AGE_*` レーティングが通常 product path の mapper 出力にならないことと、merged CAS state が Programs provider data に保持されることを追加確認した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi3
- Programs CAS代替参照 の 現在診断情報の完備 判定を、理由文字列だけでなく Rust 由来の `pmtPidResolved` / `pmtParsed` / `caStateResolved` / `freeCaModeResolved` 明示 フィールド で判定するようにした。
- Rust 公開可否診断 から PMT/CAS/free_CA_mode 解決状態を JNI/TIS へ公開した。
- 現在番組 identity が変わった場合に一時 unblock key を明示破棄するようにし、同一番組・同一 レーティング 限定の unblock 条件をテストで固定した。
- 診断情報完備を示す明示フィールド と 現在番組 change unblock 破棄の 受け入れテストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi2
- `onUnblockContent()` を 現在番組 の レーティング 一致かつ event/start/end identity 完備時だけ一時 unblock するように修正した。
- Programs CAS代替参照 の 現在診断情報の完備 判定を厳格化し、不完全 診断 + 既存 scrambled channel 状態の維持を `MERGED_CHANNEL_CAS_STATE` として明示した。
- AOSP ISDB レーティングの境界値、未対応値、UNRATED 代替の視聴許可解除 禁止、merged CAS state の 受け入れテストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi
- Programs CAS 状態 代替処理 を追加し、current診断 欠落または不完全時に既存 channel `internal_provider_data` から `requiresCas` / `unsupportedCas` / `clearLivePlaybackSupported` / `channelRegistrationReady` / `epgPublishable` を復元して Programs 側へ保存するようにした。
- 視聴年齢制限の Programs 投影と Live session enforcement を r51 対応宣言対象として固定し、AOSP system-defined ISDB レーティング（`com.android.tv / ISDB / ISDB_4..20`）へ変更した。
- custom ARIB_JP / AGE_* レーティング projection を通常 product path から廃止した。
- parental block 時は `notifyContentBlocked()` + AV停止を主通知とし、parental block 理由で `notifyVideoUnavailable()` を呼ばないようにした。
- CAS 未完成 / scrambled unsupported の video unavailable reason を `VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN` に固定した。

## r50bh
- TIS の readiness 判定は、ローカル再計算ではなく Rust の `channelRegistrationReady` / `epgPublishable` / `clearLivePlaybackSupported` 診断情報を消費する形へ切り替えた。
- 登録済みまたはEPG公開可能なサービスについて、CAS未対応のスクランブルサービスを含めて Programs を公開する一方、スクランブルサービスは平文ライブ視聴成功対象から除外した。
- Stored `requiresCas`, `unsupportedCas`, `clearLivePlaybackSupported`, `channelRegistrationReady`, and `epgPublishable` in channel `internal_provider_data`.
- ライブセッションまたはスキャンが動作中の場合は起動時EPG同期を延期し、ライブセッション数が0へ戻った後、または妨げになっているスキャン/保守処理の完了後に、保留中の起動時EPG同期を再試行するようにした。
- Android/Soong build, instrumentationテスト, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bg2
- background channel maintenance の r51 必須実装条件を満たすため、boot EPG sync 成功後に background maintenance 起動を試行する接続を追加した。
- background channel maintenance は scan/maintenance 実行中または active ライブセッション 存在時には開始せず、skip 理由を `BackgroundChannelMaintenanceDiagnostics` に残すようにした。
- ライブセッション の active count を `ChannelScanManager` で管理し、maintenance 開始判定のテストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bg
- CAT-only EMM メタデータを CAS state / 診断情報に残し、PMT CA がない 非スクランブルサービスの playback は block しないまま、descrambler attach / key トークン / scrambled success へ接続しないようにした。
- empty EIT 更新区間 を JNI/TIS publish path に伝え、非空→空の EPG 更新でも obsolete `TvProvider.Programs` を削除できるようにした。
- boot EPG sync を既存 channel の p/f 最小更新に限定し、`background channel maintenance` を r51 スコープ内の必須実装として追加した。どちらも新規 channel insert は行わない。
- setup scan channel registration を サービス単位の登録可能 gate に変更し、global discovery complete 前でも 登録可能 な partial サービスは登録可能にした。
- `ProgramPublishResult.changed` に deleted を含め、delete-only update を変化として扱うようにした。
- `tis/DESIGN_JA.md` を r51 の boot/background maintenance と サービス単位の登録可能 方針に合わせて改訂した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf2
- r50bf のロジック未達を是正し、decoder oversized sample drop 時に MediaCodec input buffer を zero-size queue で必ず返すようにした。
- AudioTrack write を blocking write + bounded zero-write retry に変更し、positive partial write 後に一時的な 0 write が返っても残り PCM を破棄しないようにした。
- ARIB broadcast genre の期待値更新に合わせ、TIS 側テストの genre トークン を `<majorName>/<middleName>` 形式へ更新した。
- `TisR51FixedPlanAcceptanceTest` の `SiCollectionResult` 呼び出しに `countsSignature` を追加し、r50bf の test compile 未達を解消した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf
- r51 の音声対応範囲を ARIB STD-B32 の TS 音声前提に合わせ、AAC は `stream_type=0x0f` の ADTS のみ supported とし、`0x11` は LATM/LOAS 未実装のため supported/viewable/decoder 対象から除外した。
- setup scan の channel 登録を complete discovery のみに固定し、partial discovery は診断だけに残すようにした。
- CAT-only EMM メタデータを dynamic filter 対象へ含め、CAS 仮実装 のまま descramble 成功扱いにしない境界を維持した。
- TvProvider Programs 更新で、同一 channel/サービスの今回 EPG 更新区間 内にある obsolete event row を削除するようにした。
- `TvTrackInfo` language を `Locale` と最小 alias map で ISO 639-2/T へ正規化し、空文字・無効値では `setLanguage()` を呼ばないようにした。
- decoder input buffer 超過 sample の prefix queue を禁止し、sample 全体を drop + 診断カウンター に変更した。zero-size video output と AudioTrack partial write の扱いも修正した。
- dynamic receiver 登録を API 33+ で `RECEIVER_NOT_EXPORTED` 明示に統一し、parental-control action は framework 定数参照へ変更した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50be
- `tis/INTEGRATION.md` を開発規則で許可する方針に合わせ、TIS product integration 手順をリリース物として維持した。
- `tis/INTEGRATION.md` から release 固有表現を外し、product package、priv-app、Direct Boot、TIS discovery の統合確認手順に限定した。
- CHANGELOG の見出しを `# CHANGELOG` と `## r50be` 形式に統一した。

## r50bd
- r51向け Direct Boot 境界、TvProvider Programs 更新、サービス単位 CAS、AudioTrack write 診断、PTS代替同期 診断、extended event JSON 解析、TIS product integration を更新。

## r50bc6
- ライブセッション が検出した video メタデータを 現在番組 key ごとに保持し、後続の EIT 由来 Programs 再 publish でも `videoFormat` / `videoWidth` / `videoHeight` を保持するようにした。
- `MaleicacidLiveSession` の video メタデータ merge helper と、EIT 再 publish 後も `internal_provider_data` の `videoFormatB64` が消えない回帰テストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc5
- `MaleicacidLiveSession` の ライブ Programs 更新を `ProgramPublishCoordinator` 経由へ統一し、r50bc4 の証跡不一致を解消した。
- ライブ更新 の重複抑止 署名 を TvProvider 投影対象全体に広げ、description、コンテンツレーティング、unsupported descriptor、video format などの更新を落とさないようにした。
- ライブ更新 が未登録 channel の Programs を作らず、既存 channel の投影内容変更だけを upsert する回帰テストを追加した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc4
- r50bc3 完了判定で検出された test 側の `PlaybackUnavailableReason` 未定義参照を修正した。
- `ServiceListBuilder` の r51 平文視聴可能 判定を実経路から呼ぶ共通関数へ分離し、PMT/PCR/video/free_ca/CA descriptor/Rust 対応宣言可能 の反例を 受け入れテスト に追加した。
- r50bc3 で残った英語文コメントのうち、今回の修正範囲と既知指摘範囲を日本語化した。
- Android/Soong build、instrumentationテスト、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc3
- r51ビルド前レビューに向けた r50bc2 の未達として、サービス単位の平文視聴可能部分公開、ライブ Programs のデバウンス、初回フレームタイムアウト補助、section短読みに関する分岐網羅、音声master代替処理診断情報を修正した。
- ライブ更新が既存チャンネルだけを更新し、同一 EIT スナップショットで余分な `TvProvider` upsert が発生しないよう、`ProgramPublishCoordinator` を追加した。
- リリースツリーから不正なトップレベル r50bc2 リリースノートを削除した。
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb11
- ビルド前レビューで見つかった r50bb10 の実装限定未達を修正した。
- `PlaybackStartGate` に渡す前に `onSelectTrack(TYPE_AUDIO)` の再生署名を非null化し、Kotlin nullable 型不一致を避けるようにした。
- 視聴制限によるブロック処理を統一し、初回フレーム時点のブロック判定でもAV pipelineを停止し、再生署名とゲート状態をリセットするようにした。
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb10
- 視聴制限の再評価で現在番組がブロック状態へ遷移した場合に再生を停止し、レーティングが再び許可されるまで pipeline を安全な利用不可状態に保つようにした。
- `onSelectTrack(TYPE_AUDIO)` は音声専用 filter/decoder 切替経路を使うように作り直した。音声切替失敗時は全体の `PlaybackPipeline.start()` を呼んで映像を破棄せず、`false` を返して現在の再生署名を維持する。
- 視聴制限による停止動作と、音声切替失敗時の状態維持を確認する対象限定の受け入れ確認を追加した。
- Android/Soong build, instrumentation tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb7
- Android 14 の `TvInputService.onCreateSession(inputId, sessionId)` sessionId 付き overload を追加した。
- r51 TIS の視聴可能/再生選択経路から HEVC `0x24` を除外した。
- ARIB 視聴年齢制限 JNI getter、Kotlin レーティング写像、TvProvider コンテンツレーティング投影、LiveSession 視聴制限ゲートを追加した。
- 初回フレームコールバック用の再生世代ガードを追加した。
- Limited ライブ更新 program publication to existing channels and added boot-time minimal EPG sync entrypoint.
- H.264 SPS の寸法解析を追加し、AVC MediaFormat 構築時の固定 1920x1080 代替処理を削除した。
- PMT由来の trackメタデータ通知と、`onSelectTrack()` による音声切替を追加した。
- NONE 時に selector setter を使わないことで、CS110 stream selector 既定値への依存を削除した。
- Enabled セクションフィルター CRC and added PID/table/状態 ingest counters.

## r50bb4
- PMT PID を r51視聴可能サービス公開前に検出できるよう、PMT filter 開始処理を `snapshotPmtPidsForSectionFilters()` へ切り替えた。
- TvProvider のチャンネル公開を行わなくてもスクランブルサービスを診断情報上で観測できるよう、ライブ/スキャンの CAS メタデータ経路を `snapshotCaMetadataForCasDiscovery()` と `snapshotServicesForCasDiscovery()` へ切り替えた。
- サービス/PCR/video/audio/CAS の再生状態を変えない section 更新で AV 再生が再起動しないよう、`MaleicacidLiveSession` に再生署名ゲートを追加した。
- Android/Soong build, instrumentation tests, VTS, CTS, and real-device checks were not run in this environment.

## r50ba2
- `ChannelKeys.kt` を package 外の `../` source path ではなく Soong module dependency 経由で `rec` tests から参照できるよう、`maleicacid_tvinput_channel_keys_sources` filegroup を追加した。
- No TIS Kotlin implementation, resources, manifest, permissions, or product integration files were changed.
