# arib_si_engine_rs 実装規約

共通の実装規約は `../GLOBAL_CODE_CONVENTION.md` を正とする。JNIの戻り値と失敗理由は `DESIGN_JA.md` の「実行失敗と正常な空値の区別」に従う。

## JNIのスナップショット取得

- `snapshot_bulk_json` は登録表から解析器参照を取得し、登録表のロックを解放してから解析器をロックする。
- 解析器のロック内で `bulk_snapshot_json` によりJSONを生成し、ロックを解放してから結果を返す。
- JNI入口は `snapshot_bulk_json` の結果を `java_string` へ渡す。Javaの結果文字列生成と例外送出を、ロック内の処理へ移さない。

## JNIの失敗伝達

- `java_string` と `throw_si_failure` を共通入口として使用する。
- `throw_si_failure` は失敗理由の識別子と表示用メッセージを別の引数として `NativeSiException` のコンストラクターへ渡す。
