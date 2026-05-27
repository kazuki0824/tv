# CHANGELOG

## r50dz87

- r50dz86 の `-k 0` ログで出た binder_service dead_code 残件のうち、未使用関数・未使用定数・未使用 debug/test helper を削除または `#[cfg(test)]` に閉じた。
- 未読 private field は `_` 接頭辞へ整理した。
- FMQ wait の同一分岐を統合した。
