# tuner_hal2 failure-injection tests 後続計画

## 1. 位置づけ

本ファイルは、tuner_hal2 の rollback / cleanup / mandatory diagnostic failure composition に対する failure-injection tests を後続作業へ切り出すための計画である。

本件は現行リリースの実装完了判定根拠ではない。現行リリースでは、failure composition 実装と静的確認を対象とし、failure-injection tests の網羅追加は未実施として扱う。

## 2. 対象

次の failure injection を追加する。

1. `HalError::ComposedFailure` が primary / cleanup を保持すること。
2. root object open rollback で object registration / object construction failure と runtime cleanup failure が両方残ること。
3. child object open rollback で callback retain / runtime-id conversion / typed object construction failure と rollback failure が両方残ること。
4. callback registration で callback store retain / runtime callback registry record / domain commit / rollback / unhealthy marking の failure precedence が設計通りであること。
5. close cleanup で callback cleanup / domain cleanup / cleanup-failed marking failure が all-attempt され、primary cleanup failure が失われないこと。
6. Drop leak quarantine で callback store clear / domain drop leak record / callback registry unhealthy marking / public runtime unregister failure が typed cleanup failure として残ること。
7. frontend tune / scan worker で backend primary failure、snapshot restore failure、live pump cleanup failure、failure marking failure が composed failure として保持されること。
8. missing target strictness が rollback / public close / owner-loss cleanup に限定され、query / idempotent stop / best-effort telemetry へ広がっていないこと。

## 3. 非対象

- 未実装機能そのものの実装。
- VTS / 実機でしか確認できないハードウェア依存挙動の代替実装。
- failure-injection 目的で production code にテスト専用分岐を恒久追加すること。

## 4. 完了条件

1. production code の failure composition 契約を壊さず、テスト用 injection point が最小であること。
2. primary-only、cleanup-only、primary+cleanup の3系統を最低限検証すること。
3. callback / close / worker / Drop leak の必須診断 failure と best-effort telemetry failure を区別して検証すること。
4. `rustfmt`、対象 unit test、Android/Soong build、必要に応じて `atest` を実行し、未実行項目を明記すること。

## 5. 禁止事項

- 本ファイルを現行リリースの実装済み範囲、設計正本、完了判定根拠として参照しない。
- grep 結果のみを failure-injection tests 完了の根拠にしない。
- failure-injection tests 未追加を理由に、production code の primary + cleanup failure preservation を未修正のまま残さない。
