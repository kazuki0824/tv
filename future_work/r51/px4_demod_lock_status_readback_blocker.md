# px4 demod lock status readback blocker

## 状態

`r51` の未解決 blocker とする。

## 背景

対象の `px4_drv` は、`PTX_SET_CHANNEL` の処理中に driver 内部の `ops->check_lock()` をポーリングし、lock を取得できなければ `-EAGAIN`、取得できれば ioctl 成功として返す。このため driver 内部には demodulator lock を判定する手段が存在する。

一方、現行 userspace ABI には Linux DVB の `FE_READ_STATUS` / `FE_HAS_LOCK` に相当する、現在の demod lock 状態を副作用なしで読み出す read-only ioctl がない。`PTX_GET_CNR` は C/N の観測であり、demod lock status API ではない。

この制約により、`PTX_SET_CHANNEL` の過去の成功を lock の代用として保持する、または C/N 非ゼロを signal detection の代用として扱うことはできても、選局後の lock loss / relock を current status として正しく観測できない。

AOSP Tuner HAL の `FrontendStatusType::DEMOD_LOCK` および `FrontendEventType::LOCKED` / `LOST_LOCK` は現在の frontend/demodulator 状態に基づいて公開する必要があるため、過去の tune ioctl 成功履歴や C/N 値を current demod lock の恒久的な代替にしてはならない。

この blocker が未解決の間、px4 frontend の current demod lock readback を実装済み能力として扱ってはならない。最終設計では driver が持つ実際の lock 判定を HAL が副作用なく観測できるようにする。

## 必要な変更

対象 `px4_drv` に、現在の demodulator lock 状態を userspace から取得するための最小限の read-only ABI を追加する。

要件は次のとおりとする。

- channel/tune/streaming 状態を変更しない read-only ioctl とする。
- driver 内部で実際の demodulator 状態を確認する `check_lock` 相当の結果を取得できる。
- `locked` と `unlocked` を取得できる。
- I/O failure、device unavailable、観測不能を正常な `unlocked` と混同しない。
- ioctl の失敗は errno 等で HAL が backend failure / unavailable と区別できる。
- AOSP の `FrontendStatusType`、`FrontendEventType` その他の AIDL enum を driver ABI へ持ち込まない。driver 固有の単純な lock 観測値を定義し、AIDL への変換は HAL 側が所有する。
- 既存 `PTX_SET_CHANNEL`、streaming、C/N、LNB、system mode ABI の意味を変更しない。

必要な current lock 観測を超えて、汎用 register read/write ABI や AOSP 固有状態機械を kernel driver に追加しない。

## HAL側の解消条件

次をすべて満たした時点で、この blocker を解消済みとできる。

1. 対象 `px4_drv` の採用 commit/build で read-only demod lock status ABI が固定されている。
2. tuner_hal2 が active tune/scan generation に対して current lock status を副作用なく取得できる。
3. `FrontendStatusType::DEMOD_LOCK` を current driver readback から導出し、過去の `PTX_SET_CHANNEL` 成功履歴や C/N 非ゼロだけを真値として使わない。
4. lock 成立後の lost-lock と、その後の relock を観測できる。
5. generation が変わった古い lock 観測を新しい要求へ流用しない。
6. `LOCKED` / `LOST_LOCK` event を公開する場合は、generation と current lock transition に一致させる。
7. 正常 lock、未lock、lost-lock、relock、I/O失敗、世代競合の試験が固定されている。

## 非対象

- px4_drv 内部の demodulator lock 判定アルゴリズムそのものの変更
- `PTX_SET_CHANNEL` が選局時に lock 待ちを行う既存動作の撤廃
- C/N の測定方法の変更
- Android Tuner AIDL の変更
- userspace から任意の demodulator register を読み書きできる汎用デバッグ ABI
