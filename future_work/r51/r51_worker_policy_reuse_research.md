# No.12 worker policy 統一に使える Rust 標準機能・OSS 調査

対象: r50ak3 ビルド前コードレビュー No.12「release path の worker sleep / diagnostic write / cleanup error 方針が複数流派になっている」への対応候補。  
目的: 既存 OSS や Rust の言語・標準ライブラリ機能で使えるものがあるかを調査し、Android TV 14 系 Tuner HAL の Rust 同期 worker に適した選択肢を整理する。

## 1. 要求される性質

Tuner HAL の worker policy 統一では、少なくとも次が必要。

- worker を detached にしない
- `JoinHandle` を保持し、shutdown / Drop / close で join する
- stop signal を全 worker に伝えられる
- stop signal で待機中 worker を即時 wake できる
- `thread::sleep()` による interval 満了待ちを shutdown path に残さない
- abnormal exit reason を呼び出し元へ残す
- callback failure / backend failure / cleanup failure を object state と diagnostics に接続する
- Android vendor HAL の同期 Rust 実装として、不要な async runtime 依存を増やさない

## 2. Rust 標準機能

### 2.1 `std::thread::JoinHandle`

Rust 標準の `JoinHandle` は thread 終了待ちの所有権を表す。`JoinHandle` を drop すると associated thread は detach され、以後 join できなくなる。  
出典: https://doc.rust-lang.org/std/thread/struct.JoinHandle.html

#### 適用可否

適用すべき。  
Tuner HAL の worker はすべて `JoinHandle` を所有構造体に保存し、close / Drop / shutdown で join する設計にする。

#### 注意

`JoinHandle` だけでは cancellation はできない。別途 stop signal / wake primitive が必要。

---

### 2.2 `std::sync::{Mutex, Condvar}`

Rust 標準の `Condvar` は thread を待機させ、`notify_one()` / `notify_all()` で wake できる。公式 docs は、`notify_one` は待機中 thread を wake するが通知は buffer されないと説明している。  
出典: https://doc.rust-lang.org/std/sync/struct.Condvar.html

#### 適用可否

最も適用しやすい。  
Android HAL の同期 worker では、追加 crate なしで `Mutex<WorkerState> + Condvar` を使うのが安定する。

#### 必須パターン

lost wake を防ぐため、`Condvar` は必ず predicate と同じ mutex で使う。

```rust
struct WorkerSignal {
    state: Mutex<WorkerSignalState>,
    cv: Condvar,
}

struct WorkerSignalState {
    stop: bool,
    wake_generation: u64,
}
```

待機側は lock 中に `stop` / `wake_generation` を確認し、条件が満たされない場合だけ `wait_timeout` に入る。wake 側は lock 中に `stop` または `wake_generation` を更新してから `notify_all()` する。

#### 採用評価

第一候補。  
VNDK / Soong dependency を増やさず、Android HAL の non-async worker に合う。

---

### 2.3 `std::sync::mpsc`

Rust 標準の `mpsc` は multi-producer / single-consumer FIFO channel。`Sender` / `SyncSender` は clone 可能で、複数 producer から single receiver へ送信できる。  
出典: https://doc.rust-lang.org/std/sync/mpsc/

#### 適用可否

worker command queue には使える。  
ただし receiver は single-consumer なので、複数 worker に同じ stop event を broadcast する用途には直接向かない。

#### 採用評価

個別 worker へ command を送る用途なら可。  
全 worker 共通の stop/broadcast には `Condvar` または per-worker sender の管理が必要。

---

### 2.4 `std::thread::park` / `Thread::unpark`

Rust 標準の `park` / `unpark` は thread-local token による低レベル blocking support。  
出典: https://doc.rust-lang.org/std/thread/fn.park.html

#### 適用可否

使えるが、HAL worker runtime の共通 primitive には採用しない。

#### 理由

`park` token は thread 単位で共有されるため、ライブラリ内部や別処理が `park` / `unpark` を使うと wake token の所有が読みづらくなる。worker ごとの明示 state と diagnostics を持ちたい本件では、`Mutex + Condvar` の方が安全。

## 3. OSS crate 候補

### 3.1 `crossbeam-channel`

`crossbeam-channel` は bounded / unbounded channel を提供し、sender / receiver は clone・thread 間共有できる。  
出典: https://docs.rs/crossbeam-channel

#### 適用可否

使える。  
特に複数 event source を `select!` 的に扱いたい場合や、stop command / data event / timer event を channel に統合したい場合に有用。

#### 利点

- `select!` / timeout / tick 系の実装がしやすい
- std mpsc より柔軟
- sync worker と相性がよい

#### 欠点

- Android vendor tree / Soong に crate 依存を追加する必要がある
- HAL の最小依存方針と衝突し得る
- 既存コードが `Mutex + Condvar` に寄っている場合、全面置換コストが高い

#### 採用評価

追加依存を許すなら第二候補。  
ただし r51 前の安全修正では、標準機能だけで十分なため必須ではない。

---

### 3.2 `tokio-util::sync::CancellationToken`

`tokio-util` の `CancellationToken` は cancellation request を通知する token で、task は `cancelled()` Future を待てる。  
出典: https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html

#### 適用可否

今回の Tuner HAL 同期 worker には採用しない。

#### 理由

- async runtime 前提の設計に寄る
- 既存 HAL worker は `std::thread` ベース
- Tuner HAL の vendor service に tokio runtime を導入する設計変更が大きい
- r51 前の worker policy 統一には過剰

#### 採用評価

非採用。  
将来、HAL 内部を async runtime へ全面移行する設計判断をする場合のみ再検討対象。

---

### 3.3 `parking` crate

`parking` crate は `Parker` / `Unparker` を提供し、標準 `park` / `unpark` と似た機能を thread-local token より明示的な object として扱える。  
出典: https://docs.rs/parking

#### 適用可否

使えるが、必須ではない。

#### 利点

- `thread::park` より wake object が明示的
- per-worker wake primitive として読みやすい

#### 欠点

- 追加 crate 依存
- predicate 付きの状態管理は別途必要
- `Condvar` で十分実現できる

#### 採用評価

標準 `Condvar` で不十分になった場合の代替候補。  
r51 前の最小安全修正では非採用。

---

### 3.4 `loom`

`loom` は concurrent code の test に使える crate。`Condvar` / Mutex 等の同期 primitive を model checking 的に検証するために利用できる。  
出典: https://docs.rs/loom/latest/loom/sync/struct.Condvar.html

#### 適用可否

product runtime には採用しない。  
test-only dependency として、lost wake / stop race の回帰防止には有用。

#### 採用評価

test-only で採用候補。  
ただし Android tree の Rust test dependency 追加可否を別途確認する必要がある。

## 4. 推奨構成

r51 前の Tuner HAL には、追加 runtime dependency を増やさず、Rust 標準機能だけで次の内部 utility を作るのが妥当。

```rust
struct ManagedWorker {
    name: &'static str,
    kind: WorkerKind,
    signal: Arc<WorkerSignal>,
    join: Option<JoinHandle<WorkerExit>>,
}

struct WorkerSignal {
    state: Mutex<WorkerSignalState>,
    cv: Condvar,
}

struct WorkerSignalState {
    stop: bool,
    wake_generation: u64,
}

enum WorkerExit {
    Normal,
    Abnormal(WorkerError),
}

enum WorkerError {
    BackendIo { backend: String, op: String, path: String, errno: i32 },
    CallbackDead { api: String, object_id: i32 },
    RegistryInconsistent { object: String, id: i32 },
    LockPoison { name: String },
    CleanupFailed { step: String },
}
```

## 5. 固定すべき worker API

### 5.1 起動

- `spawn_managed_worker(name, kind, signal, body) -> ManagedWorker`
- `JoinHandle` を必ず `ManagedWorker` に保存する
- spawn 直後に worker ID / kind を registry へ登録する

### 5.2 停止

- `ManagedWorker::request_stop()` は `stop = true` と `wake_generation += 1` を同じ lock 内で更新する
- 更新後に `notify_all()` する
- stop は冪等にする

### 5.3 待機

- `wait_interval_or_stop(duration)` を共通化する
- `thread::sleep()` を worker body から禁止する
- `Condvar::wait_timeout()` は predicate loop で使う
- timeout と stop wake を区別する

### 5.4 終了

- worker body は `WorkerExit` を返す
- `ManagedWorker::join()` は `WorkerExit` を受け取り diagnostics に反映する
- panic は abnormal exit として扱う
- Drop は best-effort stop + bounded join diagnostics にする

### 5.5 diagnostics 接続

- worker 名
- worker kind
- object ID
- demux ID / frontend ID / DVR ID / filter ID
- exit reason
- errno / errno name
- callback API
- cleanup step
- first failure timestamp
- last failure timestamp
- failure count

を共通 record として持つ。

## 6. 結論

### 採用するべきもの

- `std::thread::JoinHandle`
- `std::sync::{Mutex, Condvar}`
- 必要に応じて `std::sync::atomic` / `Arc`
- test-only 候補として `loom`

### 採用しないもの

- `tokio-util::CancellationToken`
- async runtime 前提の worker model
- product runtime への `crossbeam-channel` 必須依存
- product runtime への `parking` 必須依存

### 最終判断

r51 前の worker policy 統一では、**Rust 標準機能だけで `ManagedWorker + WorkerSignal + WorkerExit` を内部実装する**のが最も妥当。  
理由は、Android HAL の同期 worker と整合し、依存を増やさず、`JoinHandle` detach 問題、lost wake 問題、direct sleep 問題、abnormal exit 診断問題を同時に潰せるため。
