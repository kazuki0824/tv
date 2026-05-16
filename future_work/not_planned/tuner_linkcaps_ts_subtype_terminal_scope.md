# Tuner linkCaps と TS subtype 終端制約の表現差

## 位置付け

この文書は、本製品で将来実装しない範囲を記録する。対象は Tuner HAL の `linkCaps` 表現と、`setDataSource()` の subtype 別成立条件の差である。

## 固定内容

AOSP の `linkCaps` は demux filter の main type 粒度で表現される。本製品では TS main type 間の結線能力を維持するため、TS→TS の `linkCaps` 広告を維持する。

一方で、本製品の実装は TS main type 内の subtype を同一に扱わない。`setDataSource()` は source / destination の subtype、PID、PES stream_id、PES raw 表現、destination open type を検査し、意味的に成立しない組み合わせを拒否する。AV stream type 未設定だけでは `TsPes -> AV` の接続を拒否せず、AV filter の `start()` と配送側で未設定を拒否する。

## 本製品の終端 filter

次の filter は他 filter の source として扱わない。

- AV filter
  - AV passthrough は本製品では恒久的に対応しない。
  - ライブ AV filter は non-passthrough `MediaEvent` + shared memory 経路のみを正式対応とする。
  - AV payload を通常 FMQ / EventFlag へ載せる経路は実装しない。
- RECORD filter
  - DVR record buffer と `TsRecordEvent` 用の終端 filter とする。
  - downstream の TS packet source としては扱わない。

## 残課題として扱う理由

`linkCaps` だけでは、TS main type のうち `TsRaw` / `TsSection` / `TsPes` は source として成立し得るが、`TsAudio` / `TsVideo` / `TsRecord` は本製品では終端 filter である、という subtype 粒度の差を表現できない。

この差は AOSP API の表現粒度と本製品設計の差であり、r51、r52、r53 のいずれでも追加実装対象にしない。

## 実装上の扱い

- `linkCaps` は TS→TS を維持する。
- `setDataSource()` は subtype 別の成立条件を実装側で検査する。
- AV filter または RECORD filter を source とする要求は拒否する。
- `TsPes -> TsAudio/TsVideo` は、PES が `raw=false`、明示 stream_id、destination open type と audio/video 整合ありの場合に `setDataSource()` を受ける。`configureAvStreamType()` 未実行だけでは `setDataSource()` を拒否しないが、AV filter の `start()` と配送側では AV stream type 設定済みを要求する。

## 完了判定

- この文書の存在により、`linkCaps` の main type 表現と subtype 終端制約の差が既知の not planned 項目として扱われる。
- 実装側は `setDataSource()` の subtype 別検査を正とし、`linkCaps` のみで成功可否を判断しない。
