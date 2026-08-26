# Maleicacid CAS HAL

このディレクトリは、ARIB STD-B25/B1をAndroid Media CASとTuner descramblerへ接続するvendor CAS HALを含む。

## 文書案内

- 公開契約、session、SmartCard/Yakisoba経路、鍵registryは `DESIGN_JA.md` を参照する。
- product、VINTF、init、SELinux、外部依存、ライセンス条件は `INTEGRATION.md` を参照する。
- 変更履歴と未実行確認は `CHANGELOG.md` を参照する。

現行コードはproduction CAS serviceとCAS/Tuner鍵bridgeを実装している。実機SmartCard adapter、secure credential、SELinux、VTS、放送波を検証したproductだけがimmutable capability profileを同梱してB25/B1を広告し、profileがないimageはfail-closedで全CA system IDを非対応とする。
