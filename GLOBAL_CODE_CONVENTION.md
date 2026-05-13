# 目的

このドキュメントは、プロジェクト全体のコーディング規則を記載するものである。全モジュールに共通する規則だけを置き、モジュール固有の規則は各モジュール直下の `CODE_CONVENTION.md` に置く。

## Rust

### 基本方針

Rust の `panic` は通常のエラー処理として使わない。利用者入力、デバイス入力、放送ストリーム、ファイル I/O、スレッドスケジューリング、ロック失敗、FFI失敗、Binder失敗、ハードウェア失敗 から到達し得る経路では、`panic` ではなく `Result`、`Option`、ドメインエラー、明示的な状態遷移に変換する。

リリース実行時経路 では次を禁止する。

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
- service 登録前に実行される明示的 致命的な設定検証
```

ただし、service 登録後、worker 起動後、公開API 呼び出し後の 実行時経路 は例外範囲に入れない。

`unreachable!()` を使う場合は、クライアント入力、ハードウェア入力、ファイル入力、放送入力、FFI結果 に依存せず、enum網羅match などコンパイル時またはローカル不変条件 で到達不能と説明できる場合に限る。使用箇所の直上には、なぜ到達不能なのかを日本語コメントで明記する。それ以外は `Err(...)` を返す。

### Result と エラー型

各 Rust crate の 公開実行時API は `Result<T, E>` を返す。エラー型 は、少なくとも次を区別できること。

```text
- クライアントエラー
- ライフサイクルエラー
- 利用不可
- I/O エラー
- 内部エラー
- poison済みロック
```

文字列だけに依存して上位層が分類する設計は禁止する。上位層で Binder状態、コールバック状態、diagnostics へ写像できる enum または構造体を使う。

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

`Option::unwrap()` は禁止する。`None` の意味を設計上明確にし、適切な エラーへ変換する。

```rust
let value = option.ok_or_else(|| HalError::InvalidState("必要な状態が未設定です".into()))?;
```

| `None` の意味 | 返すエラー例 |
|---|---|
| クライアントが必須値を渡していない | `InvalidArgument` |
| ライフサイクル上まだ設定されていない | `InvalidState` |
| 任意capability が存在しない | `Unavailable` |
| 内部registry 破損 | `Internal` |

### mutex poison復旧

mutex poison 復旧 は 閉鎖側失敗 として固定する。`PoisonError::into_inner()` で通常復旧して処理継続してはならない。

poison済みロック を検出した場合は、次を行う。

```text
- lock 名を log または diagnostics に出す
- poison回数 を増やす
- 対象オブジェクト / subsystem を 劣化、closed、failed のいずれかへ遷移させる
- 後続呼び出しは InvalidState、Internal、Unavailable のいずれかを返す
```

共通補助関数 を使うこと。

```rust
pub fn lock_or_fail_closed<'a, T>(
    mutex: &'a std::sync::Mutex<T>,
    name: &'static str,
) -> Result<std::sync::MutexGuard<'a, T>, HalError> {
    mutex.lock().map_err(|_| HalError::PoisonedLock(name))
}
```

HAL 以外の crate では、その crate の エラー型 へ写像してよい。

### スレッド / worker

ワーカースレッド は panic で黙って終了してはならない。worker本体 は `Result` を返す構造にする。

```rust
fn worker_main(...) -> Result<(), WorkerError>;
```

worker所有者 は次を行う。

```text
- 通常停止、エラー停止、panic停止 を区別する
- エラーをdiagnostics に記録する
- 影響を受けたobject を failed / degraded / closed にする
- 次の 公開API で エラーを返す
```

`std::thread::spawn(move || { ... })` 内で `unwrap()` を使って worker を落とすことは禁止する。`catch_unwind` を使う場合も、捕捉後に通常継続せず、対象 subsystem を 閉鎖側失敗 にする。

### FFI / native shim境界

Rust から C/C++ shim、kernel ioctl、libdmabufheap、FMQ/EventFlag、PC/SC、libaribcaption C API などを呼ぶ境界では、panic させない。libaribcaption は C API のみを使用し、独自 C/C++ shim を追加しない。

```text
- null pointer を unwrap しない
- 負のerrno を ドメインエラー へ変換する
- 返却されたfd / pointer / length を検証する
- unsafeブロック の前後で 事前条件 / 事後条件 を明示する
- FFI callback が Rust panic を C境界 へ越えないようにする
```

FFI補助関数 は原則として `Result<T, E>` を返す wrapper に閉じ込める。libaribcaption については、opaque pointer、caption cleanup、配列・文字列コピー、enum変換、duration/PTSの型化を Rust 側 安全wrapper が担い、C API の生 pointer を Kotlin / TIS アプリケーション層へ露出しない。

### パーサー / ストリーム入力

MPEG-TS、PSI/SI、ARIB 文字、EPG、DVR再生入力 など、外部入力由来の parser は panic してはならない。

```text
- 長さフィールド は必ず境界チェックする
- slice添字 は `get()` または 検査済みrange を使う
- 不正packet / section は Err または 破棄 + 診断 にする
- parser状態機械 の不正遷移は InvalidState / ParseError にする
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

### ロック順序 / callback

Rust実行時コード では次を守る。

```text
- lock を持ったまま外部 callback / Binder / FFI ブロック呼び出し を呼ばない
- lock を持ったまま別 subsystem へ再入しない
- callback payload は lock 内で snapshot 化し、lock 解放後に送る
- 複数 lock が必要なら ロック順序 を文書化する
```

### 静的確認

完了判定では、次の grep を行う。

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

除外してよいものは、`#[cfg(test)]`、`tests` モジュール、fuzz / bench、offline generator / build script、コメントのみ に限定する。除外は該当行が本当に test/offline/comment に属することを人間が確認する。

### レビュー観点

Rust 実装レビューでは次を必ず確認する。

```text
1. 公開実行時関数 が Result を返すか
2. Option / Result を unwrap していないか
3. 外部入力 の length / enum / range を検証しているか
4. mutex poison を 閉鎖側失敗 にしているか
5. workerエラーがdiagnosticsとobject状態に反映されるか
6. callback / FFI / Binder を lock 内で呼んでいないか
7. unsafeブロック の 事前条件 / 事後条件 が局所化されているか
8. 不正入力がpanicではなくエラーまたは破棄+診断になるか
9. testsだけで許可されるassert / unwrapがリリース経路に混ざっていないか
```

## Kotlin
