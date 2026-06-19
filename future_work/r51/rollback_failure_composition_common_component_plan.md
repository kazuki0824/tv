# rollback / cleanup failure 合成共通部品の後続計画

## 1. 管理対象

本ファイルは、primary failure 発生後に rollback / cleanup を実行する経路で、元エラーと rollback / cleanup 失敗をどう合成して返すかを r51 で共通化するための後続計画である。

本件は現行リリースの完了判定根拠ではない。現行リリースでは、既存の `finish_open_rollback()` で扱う root / child open rollback completion だけを実装済み範囲とし、汎用の failure / rollback 合成部品までは実装済み扱いにしない。

## 2. 背景

次のような経路が複数存在する。

```text
primary failure
  -> rollback / cleanup を実行
  -> rollback / cleanup failure も捨てない
  -> 呼び出し元へ返す Binder status を決める
```

既存の open rollback では `service_runtime::open_rollback::finish_open_rollback()` が root / child object open の post-registration failure を扱う。しかし callback registration、close cleanup、汎用 rollback では、元エラーと rollback / cleanup failure の合成規則がまだ共通部品として十分に切り出されていない。

## 3. r51で固定する対象候補

- root object open 後段失敗時の rollback status 合成。
- child object open 後段失敗時の rollback status 合成。
- callback artifact registration / domain commit 失敗時の callback rollback status 合成。
- close cleanup failure と cleanup failed 記録失敗の status 合成。

## 4. 禁止事項

- rollback / cleanup failure を `let _ =` で破棄しない。
- 個別 API body に primary failure / rollback failure の precedence をコピーしない。
- `finish_open_rollback()` の対象外経路を、open rollback 成功扱いとして実装済みにしない。
- 本ファイルを現行リリースの実装済み範囲、設計正本、完了判定根拠として参照しない。

## 5. 解決条件

r51で本件を完了扱いにするには、次を満たす必要がある。

1. primary failure と rollback / cleanup failure を表す共通結果型または helper を定義する。
2. open rollback 以外の callback registration / close cleanup 経路をその helper へ寄せる。
3. 元エラー優先、rollback failure 優先、cleanup failed 記録優先の precedence を API 群ごとではなく共通規則で固定する。
4. rollback / cleanup failure が発生しても、残りの cleanup step を必ず試行する。
5. 実ロジックテストで、primary failure 単独、rollback failure 単独、両方失敗を検証する。
