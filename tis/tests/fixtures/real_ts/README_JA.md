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
