# Maleicacid CAS plugin の製品組込み

本書はCAS pluginの配置、Soong設定、依存関係と組込み確認を説明する。採用するservice・ABI・backendの設計判断は [DESIGN_JA.md](DESIGN_JA.md)、製品全体の条件は [開発規則.md](../開発規則.md) を参照する。

## 製品の共通入口

製品のproduct makefileでは `vendor/maleicacid/tv/config/product_integration.mk` を継承し、BoardConfigでは `vendor/maleicacid/tv/config/BoardConfigVendorSePolicy.mk` をincludeする。これらの共通入口からTuner、TIS、CASの設定を取り込む。

CAS側は標準serviceの `com.android.hardware.cas`、vendor pluginの `libmaleicacid_b25_cas`、権限表を生成・配置する `fs_config_files` を製品へ追加する。BoardConfig入口はCASのvendor sepolicyと `TARGET_FS_CONFIG_GEN` の入力を追加する。

## credentialの配置

製品管理の既存ファイルを、共通product入口の継承前に指定する。

```make
MALEICACID_BCAS_KEYS_FILE := vendor/<製品管理ディレクトリ>/bcas_keys
$(call inherit-product, vendor/maleicacid/tv/config/product_integration.mk)
```

入力は空白とwildcardを含まない単一の既存ファイルにする。未指定ではcredentialを生成・配置しないため、Yakisobaの実復号に必要な入力は成立しない。初期読込みの入力検査はCAS側、構文の解釈は採用libyakisobaが行う。

配置先は `/vendor/etc/maleicacid/bcas_keys` とする。`config/config.fs` によりimage作成時にroot所有・media group・0640を設定し、既存の `sepolicy/file_contexts` により `maleicacid_bcas_credential` を付与する。読取り専用vendor領域へ固定配置する。所有者・権限は製品設定とSELinuxで制御し、CASの初期化後に入力ファイルの変更を監視しない。製品側の別のfs-config入力に同じ配置先の定義を重複させない。

`get_android_qcow2.sh`を使用するときは、取得manifestのtv revisionを検証対象のcommitへ合わせる。既定の `main` のままでは未マージのPRの実装は取得されない。`libyakisoba-cross`のSoong moduleが取得できることと、CAS package・credential・policyが製品へ入ることを個別に確認する。

## MediaCasServiceとpluginの配置

AOSP標準MediaCasServiceを製品で有効にする。採用するアーキテクチャに応じて、Maleicacid CAS pluginの共有ライブラリを次の場所へ配置する。

| アーキテクチャ | 配置先 |
|---|---|
| 64-bit | `/vendor/lib64/mediacas` |
| 32-bit | `/vendor/lib/mediacas` |

通常の `/vendor/lib[64]` 直下へは置かない。Soongのvendor共有ライブラリに `relative_install_path: "mediacas"` を指定するか、それと等価な配置結果を成立させる。

## Yakisobaのリンクと依存関係

[DESIGN_JA.md](DESIGN_JA.md) §6.2の静的リンクと内部adapterの選択に従い、最初の `yakisoba_only` 構成ではpluginライブラリのSoong設定に、`libyakisoba` の静的リンクと内部adapterへの依存を設定する。

採用ソースの内部関数の宣言・型と対応させ、静的リンクで必要なシンボルが解決することを確認する。共有ライブラリから未公開関数を呼べるという前提にしない。SmartCardを必要としない構成の条件は [DESIGN_JA.md](DESIGN_JA.md) §2・§5.1・§17に従う。

Yakisobaの認証情報は製品の固定credentialとして供給する。これはTunerが使用するMULTI2の`system_key` / CBC初期値とは別の入力であり、CAS backendが初期化とECM/EMM処理のために消費する。実値は通常のGit履歴、PR差分・レビュー本文、CIログ、公開artifact、テストfixtureへ含めない。例示と試験には実値ではなく明確なダミー値を使用する。製品ビルドではリポジトリ外または秘密管理された入力を上位の製品統合設定から供給する。

CAS側では初回読込みのファイル種別・サイズ・読取り成否を検査し、構文の解釈には採用libyakisobaを使用する。初期化後のファイル変更による再読込みや失効は行わない。実ファイルのコピー、owner/group/mode、SELinux labelなどproduct image上の配置は上位の製品統合設定へ集約し、このPRのCAS本体に同じ配置policyを重複実装しない。

## Tuner HALの内部鍵参照

`libmaleicacid_cas_key_client` はTuner HAL内部consumerへ静的リンクするC++ライブラリである。Tuner HALのRust descrambler libraryはSoongの`static_libs`でこのmoduleへ依存し、C ABIの結合操作でMediaCas session IDと同じtokenの共有参照を取得し、packetごとの読取りにはその参照を使う。plugin共有ライブラリをTuner HALへ直接リンクしない。

結合時だけ既存のUnix domain socketで認可済みのCASへ接続し、読取り専用の共有領域とCAS所有者の終了通知用ファイル記述子を受け取る。共有領域にはCASの同じsession状態を置き、Tuner用の独立した更新台帳を作らない。共有領域の作成・更新はCAS側、Tuner側は読取りと参照の解放を行う。C++側の参照は同時読取り可能とし、Rust側は所有参照を最後の使用者まで保持する。

この接続はLinuxの `memfd_create`、`F_SEAL_FUTURE_WRITE` と `pidfd_open` を使用する。製品kernelはこれらを備える構成（Linux 5.3以降）とする。CASが既に持つ書込みmappingを残して以後の書込みmappingを禁止し、Tunerへは読取り専用fdを渡す。所有者の終了確認は `pidfd` の非待機 `poll` で行い、各packetでCASへの要求を送信しない。CASが更新途中で終了した場合も読取りを無期限に待たせない。

CASの共有領域には `maleicacid_cas_slot` を付け、Tunerには読取り・mappingとCASから渡されたfdの使用だけを許可する。共有領域への書込み権限をTunerへ与えない。新しいサービスや通知専用の実行単位は追加しない。

token・参照寿命・失効時の契約は `DESIGN_JA.md`、TunerがCAS方式ごとに使用する製品固定parameterの配置は `../tuner_hal2/INTEGRATION.md` を参照する。

## 組込み確認

- pluginの `.so` が採用アーキテクチャに対応する上記の配置先に含まれることを確認する。
- AOSP `FactoryLoader` がその探索ディレクトリからMaleicacid pluginを列挙・読込みでき、`createCasFactory()` を発見できることを確認する。
- `yakisoba_only` ではpluginと内部adapterの依存関係、`libyakisoba` の静的リンクおよびシンボル解決を確認する。
- Yakisoba credentialの実値がrepository・CI出力・公開artifactへ混入せず、product imageでは上位の製品統合設定が必要な主体だけに読取りを許可していることを確認する。
- Tuner HAL serviceへ`libmaleicacid_cas_key_client`が静的リンクされ、CAS plugin processとTuner HAL process間のvendor内部socketによる結合と、読取り専用共有領域・所有者終了通知用fdの受渡しがSELinux policyで許可されることを確認する。
- CAS sessionのECM更新後に同じtokenでodd/even Ksが更新され、session close・CAS process喪失後のpacketで旧鍵が使用されないことを確認する。

設計上のABI・動作条件は [DESIGN_JA.md](DESIGN_JA.md) を参照する。実施した検証と未実施範囲の記録は [CHANGELOG.md](CHANGELOG.md) を参照する。
