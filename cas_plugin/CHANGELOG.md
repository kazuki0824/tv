# PR #108 Yakisoba認証情報を初期入力へ限定

- `DESIGN_JA.md` §6で固定の認証情報を初期化時だけ読み込む入力とし、通常ファイル・有限サイズ・読取り成否の検査とlibyakisobaへの構文解析の委譲を明記した。
- §13から認証情報ファイル由来の失効を除いた。セッション閉鎖、所有者喪失、内部異常と実処理で確定した致命的故障による失効は維持した。
- AOSPのCAS責務と公開API、採用libyakisobaの初期化処理を照合し、文書差分を確認した。本体と製品接続の追従はそれぞれPR #111・#113に分けた。Android/Soong、atest、VTS、実機確認は未実施。

# PR #108 Yakisoba完了確認項目の集約

- `DESIGN_JA.md` §6.3の完了確認項目を`../タスク完了判定の実施方法.md`へ移し、元の箇所を参照へ置き換えた。不正入力、複数EMM、対象外宛先、MAC不正、重複・拒否更新、後続ECM、同時初期化・処理の確認対象を維持した。
- 移動前後の確認対象、文書間参照と差分を確認した。実装、公開API、鍵の保持方法、ビルド設定と試験の期待値は変更していない。ビルド、単体試験、Soong、atest、VTS、実機確認は未実施。

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
