# 目的

このドキュメントは、プロジェクト全体のコーディング規則を記載するものである。全モジュールに共通する規則だけを置き、モジュール固有の規則は各モジュール直下の `CODE_CONVENTION.md` に置く。

## Rust

### 基本方針

Rust の `panic` は通常のエラー処理として使わない。利用者入力、デバイス入力、放送ストリーム、ファイル I/O、スレッドスケジューリング、lock failure、FFI failure、Binder failure、hardware failure から到達し得る経路では、`panic` ではなく `Result`、`Option`、domain error、明示的な状態遷移に変換する。

release runtime path では次を禁止する。

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
| `unwrap()` / `expect()` | 入力不正、デバイス不在、lock failure、FFI failure が process death に化けるため |
| `panic!()` | service、worker、parser 全体を落とすため |
| `todo!()` / `unimplemented!()` | 未実装機能が runtime crash になるため |
| `unreachable!()` | 放送波、hardware、Binder、file input の異常で到達し得る可能性があるため |
| `assert*` | runtime validation を crash にしてしまうため |
| `dbg!()` | production log、side effect、performance のリスクがあるため |

### 許可される例外

次の範囲では例外的に `unwrap()`、`expect()`、`assert*` などを許可する。

```text
- `#[cfg(test)]` の unit test / integration test
- `tests` module
- fuzz target
- benchmark code
- offline generator / build-time tool
- service 登録前に実行される明示的 fatal configuration validation
```

ただし、service 登録後、worker 起動後、public API 呼び出し後の runtime path は例外範囲に入れない。

`unreachable!()` を使う場合は、client input、hardware input、file input、broadcast input、FFI result に依存せず、enum exhaustive matching などコンパイル時またはローカル invariant で到達不能と説明できる場合に限る。使用箇所の直上には、なぜ到達不能なのかを日本語コメントで明記する。それ以外は `Err(...)` を返す。

### Result と error type

各 Rust crate の public runtime API は `Result<T, E>` を返す。error type は、少なくとも次を区別できること。

```text
- client error
- lifecycle error
- unavailable
- I/O error
- internal error
- poisoned lock
```

文字列だけに依存して上位層が分類する設計は禁止する。上位層で Binder status、callback status、diagnostics へ写像できる enum または構造体を使う。

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

### Option handling

`Option::unwrap()` は禁止する。`None` の意味を設計上明確にし、適切な error へ変換する。

```rust
let value = option.ok_or_else(|| HalError::InvalidState("必要な状態が未設定です".into()))?;
```

| `None` の意味 | 返す error 例 |
|---|---|
| client が必須値を渡していない | `InvalidArgument` |
| lifecycle 上まだ設定されていない | `InvalidState` |
| optional capability が存在しない | `Unavailable` |
| internal registry 破損 | `Internal` |

### Mutex poison recovery

mutex poison recovery は fail-closed として固定する。`PoisonError::into_inner()` で通常復旧して処理継続してはならない。

poisoned lock を検出した場合は、次を行う。

```text
- lock 名を log または diagnostics に出す
- poison count を増やす
- 対象 object / subsystem を degraded、closed、failed のいずれかへ遷移させる
- 後続呼び出しは InvalidState、Internal、Unavailable のいずれかを返す
```

共通 helper を使うこと。

```rust
pub fn lock_or_fail_closed<'a, T>(
    mutex: &'a std::sync::Mutex<T>,
    name: &'static str,
) -> Result<std::sync::MutexGuard<'a, T>, HalError> {
    mutex.lock().map_err(|_| HalError::PoisonedLock(name))
}
```

HAL 以外の crate では、その crate の error type へ写像してよい。

### Thread / worker

worker thread は panic で黙って終了してはならない。worker main は `Result` を返す構造にする。

```rust
fn worker_main(...) -> Result<(), WorkerError>;
```

worker owner は次を行う。

```text
- normal stop、error stop、panic stop を区別する
- error を diagnostics に記録する
- affected object を failed / degraded / closed にする
- 次の public API で error を返す
```

`std::thread::spawn(move || { ... })` 内で `unwrap()` を使って worker を落とすことは禁止する。`catch_unwind` を使う場合も、捕捉後に通常継続せず、対象 subsystem を fail-closed にする。

### FFI / native shim boundary

Rust から C/C++ shim、kernel ioctl、libdmabufheap、FMQ/EventFlag、PC/SC、libaribcaption C API などを呼ぶ境界では、panic させない。libaribcaption は C API のみを使用し、独自 C/C++ shim を追加しない。

```text
- null pointer を unwrap しない
- negative errno を domain error へ変換する
- returned fd / pointer / length を検証する
- unsafe block の前後で precondition / postcondition を明示する
- FFI callback が Rust panic を C boundary へ越えないようにする
```

FFI helper は原則として `Result<T, E>` を返す wrapper に閉じ込める。libaribcaption については、opaque pointer、caption cleanup、配列・文字列コピー、enum変換、duration/PTSの型化を Rust 側 safe wrapper が担い、C API の生 pointer を Kotlin / TIS application 層へ露出しない。

### Parser / stream input

MPEG-TS、PSI/SI、ARIB 文字、EPG、DVR playback input など、外部入力由来の parser は panic してはならない。

```text
- length field は必ず境界チェックする
- slice indexing は `get()` または checked range を使う
- malformed packet / section は Err または drop + diagnostic にする
- parser state machine の不正遷移は InvalidState / ParseError にする
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

### Lock ordering / callback

Rust runtime code では次を守る。

```text
- lock を持ったまま外部 callback / Binder / FFI blocking call を呼ばない
- lock を持ったまま別 subsystem へ再入しない
- callback payload は lock 内で snapshot 化し、lock 解放後に送る
- 複数 lock が必要なら lock order を文書化する
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

除外してよいものは、`#[cfg(test)]`、`tests` module、fuzz / bench、offline generator / build script、comments only に限定する。除外は該当行が本当に test/offline/comment に属することを人間が確認する。

### レビュー観点

Rust 実装レビューでは次を必ず確認する。

```text
1. public runtime function が Result を返すか
2. Option / Result を unwrap していないか
3. external input の length / enum / range を検証しているか
4. mutex poison を fail-closed にしているか
5. worker error が diagnostics と object state に反映されるか
6. callback / FFI / Binder を lock 内で呼んでいないか
7. unsafe block の precondition / postcondition が局所化されているか
8. malformed input が panic ではなく error/drop+diagnostic になるか
9. tests だけで許可される assert / unwrap が release path に混ざっていないか
```

## Kotlin
