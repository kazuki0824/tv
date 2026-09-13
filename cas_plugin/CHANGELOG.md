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
