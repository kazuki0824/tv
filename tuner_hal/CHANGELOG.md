# 変更履歴

## r50dz79

- r50dz78 の検証ログで残った `soft_demux` の test helper 可視性不一致を修正した。
- 未使用の `raw_config()` test helper を削除した。
- `CHANGELOG.md` を追加し、変更履歴の記録先を README_JA.md の規定と一致させた。
- 再検証スクリプトの `m` 実行に `-k 0` を追加し、複数モジュールの一次エラーをまとめて収集できるようにした。
- r50dz78 で未達の binder_service build gate は継続残件として release status に記録する。

## r50dz78

- `DESIGN_JA.md` から r50dz 番号付き作業メモ節を削除し、恒久仕様へ整理した。
- frontend / soft_demux / binder_service の build gate 修正を行った。
- ただし r50dz78 検証では `soft_demux` と `binder_service` に追加 build gate 残件が残った。


## r50dz80

- r50dz79 の `m -k 0` 検証で検出された soft_demux test の `raw_config()` 欠落を修正した。
- binder_service の `Status::new_service_specific_error` 呼び出しを Android Rust Binder の `&CStr` 契約に合わせて整理した。
- `WorkerRuntimeError` を文字列化する箇所を Debug 表示へ統一し、Display 実装を前提にしない形へ変更した。
- local filter の downcast は AIDL 生成 native wrapper 経由の `Binder<BnFilter>::downcast_binder::<FilterHal>()` 形に戻した。
- 検証スクリプトは引き続き `m -k 0` を使用し、複数モジュールの一次エラーをまとめて収集する。
