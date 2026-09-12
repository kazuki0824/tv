# r52_pr57_clearkey_boundary

- AOSP `libcasexampleimpl`と`libclearkeycasplugin`を組み込み、ClearKeyとRust B25/B1を単一default serviceへ合成するNDK薄層を追加した。ClearKey欠落時はservice登録を拒否する。
- 未知IDのplugin/descramblerとB25/B1 descramblerはAIDL成功/nullを返す。ClearKeyのprovision/ECM/event/descrambleはAOSPの実装へ委譲し、公開AIDL定義は変更しない。
- Soongの境界unit testを追加した。Android/Soong build、追加native test、CAS AIDL VTSは実行環境がなく未実施。host Rust試験はAIDL/NDK実体検証を代替しない。

# r52_pr57_failure_recovery

- AIDL CASのservice-specific statusを正の定数値へ修正し、結果不明のtimeoutをINVALID_STATEへ写像した。
- revoke/下位closeの失敗をsessionに保持し、成功済みstepを繰り返さず再試行する。plugin破棄後もservice ownerが保持し、reaperで未完了cleanupを回収する。open/release競合とsession private dataのfatal failureも同じ失効経路へ接続した。
- socket接続をnonblockingにし、送受信を含む有限deadlineを適用した。timeout/送信後切断をfallback条件から分離し、open結果不明時は試行したpathのcleanupを保持する。成功応答payload長もoperationごとに検証する。
- 検証: CAS core 17件、transport 10件のhost unit testsに成功。実socketのtimeout/切断/過大応答、cleanup再試行、Binder owner消滅相当、open/release競合を含む。Android/Soong実体build、AIDL VTS、実card/放送波確認は未実施。

# 変更履歴

## r52-implementation

- 旧CAS stub serviceをproduction `android.hardware.cas.IMediaCasService/default`へ置換し、B25 `0x0005` / B1 `0x0001`のimmutable capability snapshot、`ICas` session lifecycle、公開status写像を実装した。
- complete ECM/EMM section検証、B25 ECM/EMM、B1 ECM-only、SmartCard/Yakisobaのbounded local IPC、timeout非fallbackとsession中path不変を実装した。
- MediaCas session IDをTuner tokenに使い、CAS側adapterからgeneric Tuner key provisioningへ接続した。Tuner境界はopaque provider ID/provider generation/key epochだけを扱い、B25/B1やCA system IDを解釈しない。session open時の未解決entry予約、ECM key epoch publish、close/release/fatal failure時revoke、stale epoch・identity検証を維持した。
- raw/prepared鍵のDebug表示をredactし、鍵resource、IPC frame、ECM material、private dataをdrop/replacement時にzeroizeする境界を追加した。
- 明示`release()`に加えてBinder plugin objectのDropでも全session revoke/closeとlistener解放を試行し、Drop cleanup失敗をservice-owned飽和counterへ記録するようにした。
- Tuner HALだけがpacket descrambleを行う責務を維持し、CAS HALの`createDescrambler()`を非対応、`isDescramblerSupported()`を`false`とした。
- production Soong module、init/VINTF、product integration、generic key provisioning socket sepolicy、Rust 1.81 host workspace、GitHub-hosted `ubuntu-latest` CIを追加した。
- host Rust workspaceの`cargo check --workspace --all-targets --locked`と`cargo test --workspace --locked`を実行した。Android/Soong build、AIDL VTS、採用SmartCard adapter/secure credential、実card/放送波は未実行であり、capability profile同梱前のproduct gateとして残る。

## r52-design

- B25 `0x0005`とB1 `0x0001`のplugin、advertise gate、単一`ICas`、CAS HAL descrambler非対応を正本化した。
- B25 SmartCard、B1 SmartCard ECM-only、debug限定Yakisoba B25経路と、timeout非fallback・session中path不変を固定した。
- MediaCas session ID bytesをTuner tokenにそのまま使い、generation/key epoch/key materialを内部registryだけに保持する契約へ統一した。
- ECMのatomic key epoch commit、close/release/revoke、秘密情報、listener failure、status写像を固定した。
- VINTF/init/SELinux/SmartCard/Yakisoba IPC、build profile、Apache-2.0/GPL-3.0配布条件を`INTEGRATION.md`へ固定した。
- この設計コミット時点ではproduction実装と各試験は未着手だった。後続の実装状況は `r52-implementation` を正とする。
