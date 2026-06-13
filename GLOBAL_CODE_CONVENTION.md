# 目的

このドキュメントは、プロジェクト全体のコーディング規則を記載するものである。全モジュールに共通する規則だけを置き、モジュール固有の規則は各モジュール直下の `CODE_CONVENTION.md` に置く。

## Rust

### 基本方針

Rust の `panic` は通常のエラー処理として使わない。利用者入力、デバイス入力、放送ストリーム、ファイル I/O、スレッドスケジューリング、ロック失敗、FFI失敗、Binder失敗、ハードウェア失敗 から到達し得る経路では、`panic` ではなく `Result`、`Option`、ドメインエラー、明示的な状態遷移に変換する。

リリース実行時経路では次を禁止する。

```text
unwrap()
expect()
panic!
todo!
unimplemented!
unreachable!
assert!()
assert_eq!()
assert_ne!()
dbg!()
```

禁止理由は次の通りである。

| 対象 | 理由 |
|---|---|
| `unwrap()` / `expect()` | 入力不正、デバイス不在、ロック失敗、FFI失敗 が プロセス終了 に化けるため |
| `panic!()` | サービス、ワーカー、パーサー 全体を落とすため |
| `todo!()` / `unimplemented!()` | 未実装機能が 実行時クラッシュ になるため |
| `unreachable!()` | 放送波、ハードウェア、Binder、ファイル入力 の異常で到達し得る可能性があるため |
| `assert*` | 実行時検証をクラッシュ にしてしまうため |
| `dbg!()` | 本番ログ、副作用、性能 のリスクがあるため |

### 許可される例外

次の範囲では例外的に `unwrap()`、`expect()`、`assert*` などを許可する。

```text
- `#[cfg(test)]` の 単体テスト / 結合テスト
- `tests` モジュール
- fuzz対象
- ベンチマークコード
- オフライン生成器 / ビルド時ツール
- サービス登録前に実行される明示的 致命的な設定検証
```

ただし、サービス登録後、ワーカー起動後、公開API呼び出し後の実行時経路 は例外範囲に入れない。

`unreachable!()` を使う場合は、クライアント入力、ハードウェア入力、ファイル入力、放送入力、FFI結果 に依存せず、enum網羅match などコンパイル時またはローカル不変条件 で到達不能と説明できる場合に限る。使用箇所の直上には、なぜ到達不能なのかを日本語コメントで明記する。それ以外は `Err(...)` を返す。

### Result と エラー型

各 Rust crate の 公開実行時APIは `Result<T, E>` を返す。エラー型は、少なくとも次を区別できること。

```text
- クライアントエラー
- ライフサイクルエラー
- 利用不可
- I/O エラー
- 内部エラー
- 汚染済みロック
```

文字列だけに依存して上位層が分類する設計は禁止する。上位層で Binder状態、コールバック状態、診断情報へ写像できる enum または構造体を使う。

例:

```rust
pub enum HalError {
    InvalidArgument(String),
    InvalidState(String),
    Unavailable(String),
    NoMemory(String),
    Io(std::io::Error),
    PoisonedLock(&'static str),
    Internal(String),
}
```

### Option扱い

`Option::unwrap()` は禁止する。`None` の意味を設計上明確にし、適切なエラーへ変換する。

```rust
let value = option.ok_or_else(|| HalError::InvalidState("必要な状態が未設定です".into()))?;
```

| `None` の意味 | 返すエラー例 |
|---|---|
| クライアントが必須値を渡していない | `InvalidArgument` |
| ライフサイクル上まだ設定されていない | `InvalidState` |
| 任意 capability が存在しない | `Unavailable` |
| 内部 registry 破損 | `Internal` |

### mutex汚染復旧

mutex汚染復旧は、通常復旧ではなく各モジュールの異常時処理へ写像する。`PoisonError::into_inner()` で通常復旧して処理継続してはならない。

汚染済みロックを検出した場合は、次を行う。

```text
- ロック名をログまたは診断情報に出す
- 汚染回数を増やす
- 対象モジュールの設計文書で定義された異常時状態へ遷移させる
- 後続呼び出しのエラー種別は対象モジュールの設計文書を正とする
```

共通補助関数を使うこと。

```rust
pub fn lock_or_fail_closed<'a, T>(
    mutex: &'a std::sync::Mutex<T>,
    name: &'static str,
) -> Result<std::sync::MutexGuard<'a, T>, HalError> {
    mutex.lock().map_err(|_| HalError::PoisonedLock(name))
}
```

HAL 以外の crate では、その crate のエラー型へ写像してよい。

### スレッド / ワーカー

ワーカースレッドは `panic` で黙って終了してはならない。ワーカー本体は `Result` を返す構造にする。

```rust
fn worker_main(...) -> Result<(), WorkerError>;
```

ワーカー所有者は次を行う。

```text
- 通常停止、エラー停止、`panic` 停止を区別する
- エラーを診断情報に記録する
- 影響範囲と次状態を対象モジュールの設計文書へ写像する
- 後続APIのエラー種別は対象モジュールの設計文書を正とする
```

`std::thread::spawn(move || { ... })` 内で `unwrap()` を使ってワーカーを落とすことは禁止する。`catch_unwind` を使う場合も、捕捉後に通常継続せず、対象モジュールの設計文書で定義された異常時状態へ遷移させる。

### FFI / ネイティブ薄層境界

Rust から C/C++ 薄層、kernel ioctl、libdmabufheap、FMQ/EventFlag、PC/SC、libaribcaption C API などを呼ぶ境界では、`panic` させない。libaribcaption は C API のみを使用し、独自 C/C++ 薄層 を追加しない。

```text
- nullポインターを `unwrap` しない
- 負の errno を ドメインエラーへ変換する
- 返却されたfd / pointer / length を検証する
- `unsafe` ブロックの前後で 事前条件 / 事後条件を明示する
- FFI コールバックが Rust `panic` を C境界 へ越えないようにする
```

FFI補助関数は原則として `Result<T, E>` を返す ラッパーに閉じ込める。libaribcaption については、不透明ポインター、字幕リソース解放、配列・文字列コピー、enum変換、duration/PTSの型化を Rust 側の安全ラッパーが担い、C API の生 pointer を Kotlin / TISアプリケーション層へ露出しない。

### パーサー / ストリーム入力

MPEG-TS、PSI/SI、ARIB 文字、EPG、再生キュー入力など、外部入力由来の parser は `panic` してはならない。

```text
- 長さフィールドは必ず境界チェックする
- slice添字は `get()` または 検査済み range を使う
- 不正 packet / section は Err または 破棄 + 診断にする
- parser 状態機械の不正遷移は InvalidState / ParseError にする
```

禁止例:

```rust
let pid = ((buf[1] as u16 & 0x1f) << 8) | buf[2] as u16;
```

長さ確認後の例:

```rust
if buf.len() < 188 {
    return Err(ParseError::ShortPacket { len: buf.len() });
}
```

### ロック順序 / コールバック

Rust実行時コード では次を守る。

```text
- ロックを持ったまま外部 コールバック / Binder / FFI ブロック呼び出し を呼ばない
- ロックを持ったまま別 サブシステム へ再入しない
- コールバックペイロードは ロック内で snapshot 化し、ロック解放後に送る
- 複数ロック が必要なら ロック順序 を文書化する
```


### Rust test / loom test の分担

Rustで実装し Android.bp を持つモジュールは、通常の Rust 単体テストと loom テストを分ける。

通常の Rust 単体テスト:

- Soong の rust_test として定義する。
- atest で実行可能にする。
- parser、AIDL入力変換、resource ledger、runtime state、status mapping、公開関数の戻り値と状態遷移を検査する。
- cfg(loom) を有効にしない。
- libloom に依存しない。
- Android.bp に存在しない #[test] を完了条件に数えない。

loom テスト:

- 並行性、lock順序、interleaving、race 条件の検査だけを対象にする。
- ビルドホスト側で実行する。
- 通常の rust_test に混ぜない。
- target device 上の atest、VTS、実機確認の代替にしない。
- cfg(loom) と libloom 依存は loom 専用 test module に限定する。
- loom 専用 test module は通常の rust_test module と名前を分ける。
- production module に loom defaults を適用しない。

禁止:

- 通常 rust_test に cfg(loom) を混ぜること。
- loom テストを atest / VTS / 実機確認の代替完了条件にすること。
- 同じテストを通常 rust_test と loom test の両方の正本にすること。

### 実装規約レビューの補助確認

実装規約レビューでは、補助確認として次の grep を行う。本節は完了判定の正本ではなく、完了判定は `タスク完了判定の実施方法.md` を正とする。

```bash
grep -RIn --include='*.rs' \
  -e 'unwrap()' \
  -e 'expect(' \
  -e 'panic!' \
  -e 'todo!' \
  -e 'unimplemented!' \
  -e 'unreachable!' \
  -e 'assert!' \
  -e 'assert_eq!' \
  -e 'assert_ne!' \
  vendor/maleicacid/tv
```

除外してよいものは、`#[cfg(test)]`、`tests` モジュール、fuzz / bench、オフライン生成器 / ビルドスクリプト、コメントのみ に限定する。除外は該当行が本当に test/offline/comment に属することを人間が確認する。

### レビュー観点

Rust 実装レビューでは次を必ず確認する。

```text
1. 公開実行時関数 が Result を返すか
2. Option / Result を `unwrap` していないか
3. 外部入力の length / enum / range を検証しているか
4. mutex汚染を対象モジュールの異常時状態へ写像しているか
5. ワーカーエラーが診断情報と対象モジュールの状態へ反映されるか
6. コールバック / FFI / Binder を ロック内で呼んでいないか
7. `unsafe` ブロックの 事前条件 / 事後条件 が局所化されているか
8. 不正入力がpanicではなくエラーまたは破棄+診断になるか
9. testsだけで許可されるassert / unwrapがリリース経路に混ざっていないか
```

## Kotlin
