# Maleicacid CAS plugin の製品組込み

本書はCAS pluginの配置、Soong設定、依存関係と組込み確認を説明する。採用するservice・ABI・backendの設計判断は [DESIGN_JA.md](DESIGN_JA.md)、製品全体の条件は [開発規則.md](../開発規則.md) を参照する。

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

Yakisobaの認証情報は製品の固定入力として供給する。所有者・アクセス権の設定は製品の配置設定とSELinuxに集約し、CAS側では初回読込みのファイル種別・サイズ・読取り成否を検査する。構文の解釈には採用libyakisobaを使用し、初期化後のファイル変更による再読込みや失効は行わない。

## 組込み確認

- pluginの `.so` が採用アーキテクチャに対応する上記の配置先に含まれることを確認する。
- AOSP `FactoryLoader` がその探索ディレクトリからMaleicacid pluginを列挙・読込みでき、`createCasFactory()` を発見できることを確認する。
- `yakisoba_only` ではpluginと内部adapterの依存関係、`libyakisoba` の静的リンクおよびシンボル解決を確認する。

設計上のABI・動作条件は [DESIGN_JA.md](DESIGN_JA.md) を参照する。実施した検証と未実施範囲の記録は [CHANGELOG.md](CHANGELOG.md) を参照する。
