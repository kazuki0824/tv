# 日本の高度放送向け MMTP / TLV 対応

## 位置付け

この文書は、現行製品で採用しない将来拡張範囲を記録する。現行製品の Tuner HAL は MPEG-2 TS 系の日本向け放送を対象とし、demux capability / filter capability / VTS profile は TS-only とする。この方針の正本は `tuner_hal/DESIGN_JA.md` および製品スコープを定める文書であり、本書は現行仕様を再定義しない。

日本の現行2K放送が終了すること自体を確定済みの移行条件とはしない。将来、製品対象を高度広帯域衛星デジタル放送（ISDB-S3）または高度地上デジタルテレビジョン放送へ拡張し、あるいは現行TS系の対象をそれらへ置き換える場合に、本項目を再評価する。

## 将来対応が必要になる理由

ARIB STD-B44 は高度広帯域衛星デジタル放送の伝送方式 ISDB-S3 を規定し、TLV をその伝送・多重化体系に含む。ARIB STD-B60 はデジタル放送における MMT（MPEG Media Transport）による映像・音声・データ等のメディアトランスポート方式を規定している。また、STD-B60 2.0 および STD-B32 4.0 以降には高度地上デジタルテレビジョン放送に関する規定が追加され、STD-B32 第3部は TLV packet による伝送を含む多重化方式を扱う。

Android Tuner HAL AIDL V2 の `DemuxFilterMainType` は `TS` に加えて `MMTP`、`IP`、`TLV`、`ALP` を持つ。したがって、日本の高度放送を製品対象へ追加する場合、現行の TS-only capability をそのまま流用して対応済みとみなしてはならない。少なくとも実際に採用する放送方式と transport graph に基づき、`MMTP` / `TLV`、必要なら `IP` を含む main type と subtype の対応範囲を新たに設計する必要がある。

## 再設計対象

高度放送対応を採用する場合は、少なくとも次を同じ設計変更として閉じる。

- `DemuxCapabilities.filterCaps` / `linkCaps` の main type 広告と、広告した pair の実接続能力
- `openFilter()` の MMTP / TLV / IP subtype acceptance と、未対応 subtype の戻り値
- `setDataSource()` を用いる filter chaining の必要性、接続方向、lifecycle、世代、rollback、queue / assembler 境界
- MMTP / TLV / IP の parser、packet boundary、continuity / loss、FMQ / callback / event の所有関係
- MMT による映像・音声・データ配送と、AOSP AV filter / `MediaEvent` への写像
- SI / service / program 情報の取得と TIS / TvProvider への投影
- 録画、再生、descrambler / CAS、A/V sync、timestamp の方式差分
- frontend / backend が高度放送の物理伝送方式を実際に受信・検証できることの capability snapshot への反映
- 対応する VTS artifact / variant / profile と、広告 capability から到達できる試験経路

## 固定しない事項

現時点では、次を将来仕様として先取りして固定しない。

- `linkCaps` の具体値または `TLV -> MMTP` 等の具体的な接続 graph
- `MMTP` / `TLV` / `IP` のどの subtype を必須対応とするか
- 高度衛星放送と高度地上放送で同一の demux graph を使用すること
- 現行 backend が追加 hardware / driver 変更なしで対応可能であること
- 高度放送対応時にも現行 TS-only parser / SI / AV / record 経路をそのまま再利用できること

これらは、対象とする放送規格の版、受信 hardware / driver、Android Tuner HAL / VTS の対象版、製品要件を固定した時点で設計する。

## 非採用範囲の管理境界

- 現行製品では `MMTP` / `TLV` / `IP` / `ALP` を対応 capability として広告せず、TS-only 方針を維持する。
- 本書の存在を理由に、未実装 main type を VTS profile や `filterCaps` / `linkCaps` へ先行して追加しない。
- 日本の高度放送を製品スコープへ追加する判断が行われた場合、本書を単なる実装 TODO として消化せず、放送規格から AOSP-facing capability、data path、TIS までを横断する設計変更として再評価する。
