# PR #113 固定認証情報の監視を接続経路から除去

- 共有参照の結合を初期化済み状態と既存の鍵状態の判定へ限定し、認証情報ファイルの再検査を除いた。受付処理の定期監視と`pollCredential()`を削除した。
- 共有領域の読取り専用参照、CAS所有者終了用の`pidfd`、セッション・EMM由来の失効、通信の有限待機と受付異常時の失効は維持した。
- 共有参照の試験を初期化後の入力削除に依存しない結合・読取りへ変更し、閉鎖後の参照拒否も確認対象とした。組込み手順から入力監視の記述を除いた。
- CIで判明した応答分類試験の旧期待値を訂正し、ファイル削除ではなく実際の全失効後に再結合が拒否されることを確認するよう変更した。
- ホストの本体試験12組が成功。通信試験はGitHub Actionsで検証する。親PRの固定入力本体の変更はPR #111、設計正本の変更はPR #108に分けた。Android/Soong全体、atest、VTS、実機確認は未実施。

# PR #113 CAS所有の共有参照への製品接続

- CASの既存session状態を共有領域へ置き、Tunerは結合時に取得した読取り専用参照を使用するよう変更した。共有領域の更新・失効と所有者終了の局所確認により、TISの通知や再結合を待たず次のpacketへ反映する。
- Unix domain socketは参照の結合だけに使用し、ライブ入力とPlayback DVRからpacketごとの外部照会を除いた。CAS内部の失効契約とECM/EMM処理は維持し、別の鍵台帳や通知専用workerは追加していない。
- 共有参照の受渡しに必要なSELinux設定と組込み手順を追加した。旧読取り入口は既存試験専用に隔離した。
- ローカルでC++本体と共有領域の試験12組が成功。[CAS CI](https://github.com/kazuki0824/tv/actions/runs/34873809815)は依存取得のHTTP 503を再実行し、通常構成とASan/UBSan構成で各26組成功。共有領域の書込み禁止と世代上限の試験も追加した。Android/Soong、atest、VTS、実機確認は未実施。

# PR #111 固定認証情報の読込みを簡素化

- 認証情報ファイルの所有者・権限の再検査、独自の構文解析、初期化後の変更監視を削除した。通常ファイル、空でない有限サイズ、シンボリックリンク拒否、読取り量の上限とI/O失敗の拒否は維持した。
- 初期化済み状態はディスクへ再依存せず、ECM・EMM・鍵参照は既存の処理結果と鍵状態を使用する。致命的故障時の全失効、EMM更新とセッション閉鎖による失効は維持した。
- 既存試験を初期入力の欠落・種類・サイズ境界、libyakisobaの大小文字・区切り文字・同一登録先の更新、初期化後の削除・権限変更、後続ECM/EMMと閉鎖の期待値へ追従した。製品向け組込み文書も同期した。
- ホストの本体試験12組が成功。通信試験はGitHub Actionsで検証する。Android/Soong全体、atest、VTS、実機確認は未実施。

# PR #108 Yakisoba認証情報を初期入力へ限定

- `DESIGN_JA.md` §6で固定の認証情報を初期化時だけ読み込む入力とし、通常ファイル・有限サイズ・読取り成否の検査とlibyakisobaへの構文解析の委譲を明記した。
- §13から認証情報ファイル由来の失効を除いた。セッション閉鎖、所有者喪失、内部異常と実処理で確定した致命的故障による失効は維持した。
- AOSPのCAS責務と公開API、採用libyakisobaの初期化処理を照合し、文書差分を確認した。本体と製品接続の追従はそれぞれPR #111・#113に分けた。Android/Soong、atest、VTS、実機確認は未実施。

# PR #108 Yakisoba完了確認項目の集約

- `DESIGN_JA.md` §6.3の完了確認項目を`../タスク完了判定の実施方法.md`へ移し、元の箇所を参照へ置き換えた。不正入力、複数EMM、対象外宛先、MAC不正、重複・拒否更新、後続ECM、同時初期化・処理の確認対象を維持した。
- 移動前後の確認対象、文書間参照と差分を確認した。実装、公開API、鍵の保持方法、ビルド設定と試験の期待値は変更していない。ビルド、単体試験、Soong、atest、VTS、実機確認は未実施。

# 製品固定parameterの所有と非公開入力

- `KeyClient.h`から固定MULTI2 parameterの定義を除き、CAS読取り口が動的odd/even Ksだけを返す構成にした。固定parameterはTunerの非公開製品入力として扱う。
- Tunerの入力配置手順へ`INTEGRATION.md`から参照を追加し、token・失効契約の重複説明を設計正本への参照に置き換えた。
- CAS CIの製品設定検査をTunerの入力と権限表へ拡張し、workflow表示名を日本語へ揃えた。
- ローカルのCAS本体試験は11 suite成功。socket結合は実行環境の制限によりローカルで未実施。Android/Soong、atest、VTS、実機の検証は未実施。

# Tuner descramblerへの製品鍵参照接続

- `libmaleicacid_cas_key_client` にC ABIの読取り口を追加し、Tuner HALのRust実装から標準MediaCas session IDと同じ16-byte tokenでcurrent odd/even Ksを取得できるようにした。
- token形式不正、失効・未登録token、CAS内部経路の利用不能を応答で区別し、失敗時の出力鍵をゼロ化する。
- packet単位の鍵取得とC ABIの成功・unknown-token動作を既存socket結合試験へ追加した。ローカルではsocket syscallが実行環境に拒否されるためcore 11 suiteのみ成功し、socket結合試験はGitHub Actionsで確認する。

# CASの共通製品入口への接続

- 共通product入口から標準CAS service、vendor plugin、fs-config生成物を取り込み、共通BoardConfig入口からCASのSELinux policyとcredential用fs-config入力を取り込む。
- 製品管理のcredential入力を指定した場合のコピー先を接続し、存在しない入力と複数ファイル指定をbuild設定の評価時に拒否する。image上の所有者・group・modeをroot:media、0640へ設定する。
- 製品管理ファイルの指定、未指定時の制限、検証対象revisionの選択をINTEGRATION.mdに集約した。
- Tunerのtoken解決・packet単位の鍵更新／失効の接続はこの変更では未実装。Android/Soong build、atest、VTS、実機imageでの権限確認は未実施。

# PR #108 文書責務の整理

- `INTEGRATION.md` を追加し、pluginの32-bit／64-bit配置先、通常のvendorライブラリ直下へ置かない条件、Soongの配置設定、静的リンク・内部adapterへの依存設定、シンボル解決とFactoryLoaderによる発見の確認を集約した。
- `DESIGN_JA.md` §1・§6.2・§17・§18の統合設定・確認を参照へ置き換えた。標準MediaCasService、独自serviceを持たない方針、CasPlugin ABI、静的リンクと内部adapterの設計判断、SmartCard非依存条件を維持した。
- `README_JA.md` の「現行境界」「目標構成」に重複していた設計説明を除き、設計・統合・変更履歴の文書への案内にした。
- 文書間の参照経路、移動前後の要件、重複する配置指定と差分を確認した。実装・ビルド設定・テスト期待値の変更はない。build、unit test、Soong、atest、VTS、実機確認は未実施。

# PR #108 初期容量通知とsession生成の順序

- B25/B1は固定上限の有無にかかわらず初期容量を通知し、最初のsession要求まで通知を遅延しない契約にした。固定上限なしはAOSP TRMの既定容量と同じ値を通知し、TIS側の容量分類を不要にした。
- 初期通知処理後にだけsessionを要求するTISの非同期待機・期限・失効へ正本参照を接続し、結合確認条件を追加した。
- 設計文書のみの変更。CAS本体・r52 TIS接続の実装、build、unit test、Soong、atest、VTS、実機確認は未実施。

# PR #108 有限session容量のTRM通知

- 一律に通知しない方針を訂正し、有限の同時session上限はAOSPのsession数通知でTRMへ伝える契約にした。通知なしは固定session数上限を持たない構成に限定した。
- 同一CA system全体の有効上限の正本、初期・変更通知、非同期反映との競合時の受付拒否を整理した。独自の優先度調停器や通知serviceは要求しない。
- B25各profile/B1のadvertise・結合確認条件とTISの初期通知受信順序を同期した。
- 設計文書とAOSP StatusEvent / MediaCas / TRMの処理を照合し、文書差分を確認。CAS本体・r52 TIS接続の実装、build、unit test、Soong、atest、VTS、実機確認は未実施。

# PR #108 token・鍵状態・TRM契約の補完

- session IDの初回生成・再割当てでAOSP Tunerの予約値`[0x00]`を除外し、長さ・衝突検査を公開前の条件にした。
- 動的鍵状態の登録・更新・参照・失効の責任主体と結果を明記し、CAS owner喪失、Tuner再起動、鍵状態自体の喪失を区別した。transportや追加serviceは固定していない。
- B25全profile/B1のsession数通知をAOSPの通知なし既定動作へ固定し、実資源の枯渇拒否とTRMの役割を区別した。
- CA system IDの対応を開発規則へ集約し、factory/TIS/結合検証の参照を同期した。Tuner VTSの適用範囲はTuner正本へ接続した。
- AOSP Android 14/15のAPI・VTSソース、既存TIS定数・Tuner参照処理と文書を照合。設計のみの変更で、CAS実装、build、unit test、Soong、atest、VTS、実機確認は未実施。

# PR #108 固定値・動的鍵状態の責務同期

- 複数moduleに跨る固定MULTI2 parameterと動的Ks状態の製品方針を`../開発規則.md`へ集約した。CAS pluginはTuner HALへ直接依存せず、具体的な共有方式を必須化しない。
- B25/B1の構成、ABI内部責務、SmartCard、advertise gate、ECM成功条件、失効、検証、実装順序、READMEをsession/tokenとcurrent Ksの対応・更新・失効へ統一した。
- 固定値のruntime配送と全materialのpublishを要求する表現を除き、Tuner設計の参照・復号責務と同期した。
- 設計文書のみの変更。staticな責務・参照・差分整合を確認。CAS plugin本体・鍵共有機構の実装追加、build、unit test、Android/Soong、atest、VTS、実機確認は未実施。

# PR #108 鍵共有の責務境界

- CAS pluginとTuner HALの直接接続、Unix domain socket、Tuner側server／plugin側client、接続単位の唯一の更新入口という必須構造を除いた。
- session tokenと現在の鍵状態の対応、Ks更新・失効の一貫性、所有者喪失時の失効、未許可更新と古いtokenの拒否を共有方式に依存しない契約へ整理した。共有サービス・TEE・共通backend等の採用を可能とし、新設は要求しない。
- `system_key`と`cbc_initial_value`をTuner側の固定値、odd/even KsをECMに伴う更新対象として区別した。SmartCard初期化応答からの固定値供給と、固定値を含む全materialの実行時転送を要求する記述を除いた。
- B25/B1共通ABI、Yakisoba、B1、token、ECM成功条件、責務表、検証条件を同じ鍵状態の意味に揃えた。責務表のplugin共有範囲を同一CasController内と明記した。
- AOSP参照とREADMEの変更履歴への案内を追加した。
- 設計文書のみの変更。CAS plugin本体・鍵共有機構の実装追加、build、unit test、Android/Soong、atest、VTS、実機確認は未実施。
