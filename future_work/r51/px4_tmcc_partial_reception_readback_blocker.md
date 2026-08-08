# px4 TMCC partial-reception readback blocker

## 状態

`r51` の未解決 blocker とする。

## 背景

Android 14 Tuner AIDL V2 の ISDB-T settings には `partialReceptionFlag` があり、明示値を成功させる場合は、その要求を対象 backend/device が意味的に満たしたことを設定または観測によって確認できなければならない。入力値を backend request から捨てたまま `SUCCESS` を返してはならない。

対象の `px4_drv` (`feat/android-ddk` 系) は TC90522 demodulator を使用しており、driver 内部では TC90522 register を読み出せる。しかし現行 userspace ABI は channel 設定、streaming、C/N、LNB、system mode 等に限られ、TMCC の partial-reception 状態を userspace へ返す read-only ioctl を持たない。このため tuner_hal2 からは、TC90522 が受信した ISDB-T TMCC の partial-reception flag を観測できない。

この blocker が未解決の間、px4 frontend について明示 `partialReceptionFlag` を観測不能のまま受理して `SUCCESS` にしてはならず、当該値を実装済み capability として扱わない。

## 必要な変更

対象 `px4_drv` に、TC90522 が受信した TMCC の partial-reception 状態を userspace から取得するための最小限の read-only ABI を追加する。

要件は次のとおりとする。

- channel/tune 状態を変更しない read-only ioctl とする。
- 少なくとも、現在ロックしている ISDB-T transport の TMCC partial-reception flag を取得できる。
- 未ロック、TMCC未確定、I/O失敗を正常な `false` と混同しない。
- ioctl の失敗は errno で区別でき、HAL が `UNAVAILABLE` / backend failure 等の既存公開status契約へ写像できる。
- ABIの値はAOSP enumをdriverへ持ち込まず、driver固有の単純な観測値として定義する。AIDL値への変換はHAL側が所有する。
- 既存の選局、streaming、C/N、LNB ABIの意味を変更しない。

layerごとの segment count を同一TMCC readbackで取得できるようにしてもよいが、`partialReceptionFlag` blockerの解消に不要な制御ABIや汎用register read ioctlへ拡張しない。

## HAL側の解消条件

次をすべて満たした時点で、この blocker を解消済みとできる。

1. 対象 `px4_drv` の採用commit/buildでread-only TMCC ABIが固定されている。
2. tuner_hal2 がそのABIを用いてロック後のpartial-reception状態を読み戻せる。
3. 明示 `partialReceptionFlag=TRUE/FALSE` について、要求値と観測値の一致時だけ成功し、不一致・未確定・読出失敗をsilent successにしない。
4. tune/scan generationが変わった古いreadbackを新しい要求へ流用しない。
5. 正常系、不一致、未ロック、I/O失敗、世代競合の試験が固定されている。

## 非対象

- TC90522の復調方式そのものの変更
- ARIBのpartial reception仕様変更
- Android Tuner AIDLの変更
- px4の明示segment数制御の新設
- userspaceから任意のTC90522 registerを読み書きできる汎用デバッグABI
