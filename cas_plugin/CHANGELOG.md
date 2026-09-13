# PR #108 鍵共有の責務境界

- CAS pluginとTuner HALの直接接続、Unix domain socket、Tuner側server／plugin側client、接続単位の唯一の更新入口という必須構造を除いた。
- session tokenと現在の鍵状態の対応、Ks更新・失効の一貫性、所有者喪失時の失効、未許可更新と古いtokenの拒否を共有方式に依存しない契約へ整理した。共有サービス・TEE・共通backend等の採用を可能とし、新設は要求しない。
- `system_key`と`cbc_initial_value`をTuner側の固定値、odd/even KsをECMに伴う更新対象として区別した。SmartCard初期化応答からの固定値供給と、固定値を含む全materialの実行時転送を要求する記述を除いた。
- B25/B1共通ABI、Yakisoba、B1、token、ECM成功条件、責務表、検証条件を同じ鍵状態の意味に揃えた。責務表のplugin共有範囲を同一CasController内と明記した。
- AOSP参照とREADMEの変更履歴への案内を追加した。
- 設計文書のみの変更。CAS plugin本体・鍵共有機構の実装追加、build、unit test、Android/Soong、atest、VTS、実機確認は未実施。
