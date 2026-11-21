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

## Section burstの回帰範囲

`realTsBurstLossAndRetransmissionStayWithinCandidate`は同じSHA-256のTSを実HAL実行器へ入力し、固定したsection参照値を照合してから既存SDK Filter Shadowのread/callbackへ接続する。製品Controller/SectionIngestController/実SI JNI/SiCollectionRequirements/collectionOutcomeを実行する。16件のEIT先行入力とowner control占有に続けてPID別callbackを並行実行し、ピーク16件、拒否件数、未完成→再送後完成、同候補の喪失保持、Filter交換と候補切替での旧入力拒否を検査する。

`section-burst.txt`では、入力hash・section数・PID数とhostの経過時間を、assert済みの固定期待値から区別する。`assertedExpectations`のピーク16、拒否数、overflow 0、完成状態は試験assertが検証した期待値であり、連続観測による実測レポートではない。`observedHostWallTime`だけがSystem.nanoTimeによる当該host試験の経過時間で、意図的なowner占有と逐次再投入を含む。再送は各入力の完了を待つ決定的試験であり、放送の実到着率・RF受信・実機処理性能を再現する負荷試験ではない。この成功だけで16件の必要十分性、元の15,409件の再現、全環境でのSI収集成功を宣言しない。入力率、各control操作の占有時間、機種/buildと候補別診断を揃えた実機測定は未実施条件として残す。

`ChannelScanController.startSetupScan` / `collectSiForCandidate`自体は実行しない。`collectionOutcome`へ渡す最短待機・安定時間は固定値で、実scan timer、期限内の収集終端、TvProvider公開拒否、放送からの自律再送・SI再収集を検証したものではない。ここでの「候補切替」はTuner再選局と同じ旧Filter停止後reset・候補別snapshot境界の検査に限定する。実scan全体の保証が必要な場合は既存scan入口を通る別の回帰と実機証跡が必要であり、この試験から成功を推論しない。
