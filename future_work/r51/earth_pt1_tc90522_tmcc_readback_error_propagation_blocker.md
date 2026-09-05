# earth_pt1 / TC90522 TMCC readback error propagation blocker

## 状態

`r51` の未解決 blocker とする。

## 背景

`tuner_contract/DESIGN_JA.md` は、earth_pt1 / TC90522 の ISDB-T について、明示 `partialReceptionFlag=TRUE/FALSE` と layer `numOfSegment=1..13` を、lock 後に Linux DVB から読み戻した TMCC の partial-reception 状態および layer segment count と照合して要求適合を判定する設計とする。この設計自体は変更しない。

しかし、現行 Linux DVB TC90522 driver の `get_frontend()` は TMCC register の読み出しに失敗した場合でも、その失敗を userspace が確実に識別できる形で返さない経路を持つ。TMCC read に成功した場合だけ `isdbt_partial_reception` と各 layer の `segment_count` が更新されるため、userspace からは今回の選局に対する新しい正常 readback と、read failure 後に残った既存値または初期値とを確実に区別できない。

この状態では tuner_hal2 が、TMCC 未確定、I/O 失敗、または古い readback を明示要求との正常な一致として誤認する可能性を排除できない。

この blocker が未解決の間、earth_pt1 / TC90522 について明示 `partialReceptionFlag=TRUE/FALSE` および layer `numOfSegment=1..13` の readback 検証を実装済み・利用可能な機能として扱ってはならない。これは最終設計の変更ではなく、その設計を現行 driver で安全に実装するための外部依存である。

## 必要な変更

採用する Linux kernel / TC90522 driver で、今回の lock 後 TMCC 観測が正常に成立したかどうかを userspace が判定できるようにする。

要件は次のとおりとする。

- TMCC partial-reception 状態と layer segment count の読み出しについて、I/O 失敗を正常な readback と混同しない。
- 未ロックまたは TMCC 未確定を、有効な `FALSE` や segment count と混同しない。
- 既存 Linux DVB API でエラーを正しく伝播できる修正を優先し、必要性がない限り earth_pt1 専用の独自制御 ABI を増やさない。
- 独自 read-only ABI が必要になる場合も、選局状態を変更せず、AOSP enum を kernel ABI に持ち込まず、driver 固有の観測値とエラーだけを公開する。
- 既存の選局、streaming、status、property ABI の正常系の意味を不用意に変更しない。

## HAL側の解消条件

次をすべて満たした時点で、この blocker を解消済みとできる。

1. 採用する kernel/build で TC90522 の TMCC readback 成否を userspace が判別できる修正または ABI が固定されている。
2. tuner_hal2 が lock 後に、同じ tune/scan generation に属する新しい TMCC readback を取得できる。
3. 明示 `partialReceptionFlag=TRUE/FALSE` は要求値と正常 readback が一致した場合だけ成功扱いにする。
4. 明示 layer `numOfSegment=1..13` は要求値と正常 readback が layer ごとに一致した場合だけ成功扱いにする。
5. 不一致、未ロック、TMCC 未確定、I/O 失敗、古い generation の readback を silent success にしない。
6. 正常系、不一致、未ロック、TMCC 未確定、I/O 失敗、世代競合の試験が固定されている。

## 非対象

- `tuner_contract/DESIGN_JA.md` の earth_pt1 / TC90522 に対する最終的な frontend settings 契約の変更
- TC90522 の復調方式そのものの変更
- ARIB の partial reception または segment 構成仕様の変更
- Android Tuner AIDL の変更
- userspace から任意の TC90522 register を読み書きできる汎用デバッグ ABI
