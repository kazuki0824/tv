# 実TS試験データの使用方法

`test.ts`をTISとTuner HALのホスト結合試験の共通入力として使用する。元の添付名は`test.m2ts`だが、内容は188-byte MPEG-TSであるため拡張子を`.ts`として格納している。ファイル内容は元データと同一で、末尾の短いpacketも含む。

`expected.json`には入力のSHA-256、byte数、packet数、サービス情報、選択サービスの番組時刻、TSDuckで抽出したsectionのSHA-256と出現回数を収録している。試験経路・判定は`../../../DESIGN_JA.md`の「実TSによるホスト結合試験」、ホストCIへの接続は`../../../INTEGRATION.md`を参照する。

解析に使用したTSDuckは3.45-4798である。対応するLinux環境にTSDuckを導入したうえで、このdirectoryから次を実行すると、入力の概要と表の内容を確認できる。

```bash
tsanalyze --isdb --json test.ts
tstables --isdb --default-charset ARIB --pid 0 --pid 16 --pid 17 --pid 18 --pid 20 --pid 257 --pid 258 --pid 2032 --pid 8136 --xml-output tables.xml test.ts
```

section参照値は`expected.json`の`section_reference`ごとに、同ファイルの`reference_section_command`のPID／TABLE_IDを置き換えて取得できる。binary出力を`3 + section_length`で分割し、CRCを含む各section全体のSHA-256と出現回数を数える。tsduckのXMLに表示されるARIB放送時刻はJSTとして確認し、`selected_service_pf_events`のUnix millisecond値と照合する。

TSDuckは期待値の確認用であり、通常の結合試験実行時には不要である。解析用の`.deb`や生成した一時出力をcommitする必要はない。

## AVC AU・PTS試験への共通入力

同じ`test.ts`にはAVC PID `0x281`も含まれる。#116の既存試験を通常の`#[ignore]`設定のまま、#123のCIから次のコマンドで明示実行する。リポジトリrootから実行し、試験本体とAU/PTS判定は#116側を正とする。

```bash
AVC_CAPTURE_PATH="$PWD/tis/tests/fixtures/real_ts/test.ts" \
cargo test --manifest-path tuner_hal2/host_ci/Cargo.toml --locked \
  -p maleicacid-tuner-hal2-demux-host-ci --lib \
  captured_transport_frames_have_individual_timestamps -- --ignored --nocapture
```

この試験はPSI/SI結合試験と独立してCIの成功を要求する。実AU数の固定値を新しく二重定義せず、既存Rust試験の判定を使用する。結果と計数は`avc-au-pts.log`に記録する。

## 部分受信サービスの独立照合

元TSのPID 0x0010、NIT actualのtransport descriptor loopを走査すると、Partial Reception Descriptor (tag 0xFB) のpayloadは `05 98`、列挙SIDは1432だけである。`expected.json`の各serviceの`partial_reception`はこの元byte列から固定した値であり、製品parserの出力から更新しない。現行TIS policyでは、この部分受信サービスも登録可能とする。
