# ARIB SI エンジン

このディレクトリは TIS が使用する PSI/SI/EIT 解析ライブラリを Rust で提供する。

## 文書案内

- PSI/SI/EIT、ARIB文字列、provider-data、診断、公開APIの設計判断は `DESIGN_JA.md` を参照する。
- モジュール固有の実装規約は `CODE_CONVENTION.md` を参照する。
- 変更履歴は `CHANGELOG.md` を参照する。

利用開始時は `DESIGN_JA.md` の入力・出力境界と provider-data 契約を確認し、TIS 側との統合条件は `../tis/INTEGRATION.md` を参照する。
