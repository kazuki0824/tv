# AOSP `COLUMN_SCRAMBLED` の名称と `free_CA_mode` 意味差

## 位置付け

この文書は、本製品側では解消できない AOSP `TvContract` 公開APIの意味上の不整合を、not planned の upstream 課題として記録する。

対象は `TvContract.Programs.COLUMN_SCRAMBLED` および同じ説明規則を持つ `TvContract.Channels.COLUMN_SCRAMBLED` である。

## 問題

AOSP は `COLUMN_SCRAMBLED` を「番組またはチャンネルが scrambled か否か」を示す列として命名・説明している。一方で、放送規格に EIT / SDT の `free_ca_mode` が定義されている場合は、その coding をこの列へ使用することも公開契約としている。

ARIB TR-B14 / TR-B15 の日本向け運用では、SDT / EIT の `free_CA_mode` は実際のTS componentの scramble / non-scramble 状態ではなく、無料番組 / 有料番組の区分に使用する。

- `free_CA_mode=0`: 無料番組
- `free_CA_mode=1`: 有料番組

したがってARIBでは、`COLUMN_SCRAMBLED=0` を「実際に暗号化されていない」、`COLUMN_SCRAMBLED=1` を「実際に暗号化されている」と一般化できない。無料番組でもcontent protection等によりscrambleされる場合があり、実スクランブル状態とfree/pay区分は別軸である。

このため `COLUMN_SCRAMBLED` という公開名称と「scrambled or not」という説明は、AOSPが同時に要求する `free_ca_mode` codingをARIBへ適用した場合にミスリーディングである。

## 本製品契約との関係

本製品のARIB `free_CA_mode` とTvProvider標準列の投影、実スクランブル状態との意味分離は `ARIB_SI_EPG_TvProvider投影方針.md` を唯一の正本とする。本書は現行投影規則を再定義せず、AOSP公開APIの名称・説明と放送規格codingの間に上流課題が残るという理由だけを記録する。

## AOSP upstream で必要な対応

この問題は本リポジトリだけでは解消できないため、AOSP upstreamへ次のいずれかを行う必要がある。

1. `TvContract.Programs.COLUMN_SCRAMBLED` / `TvContract.Channels.COLUMN_SCRAMBLED` のJavadocを修正するパッチを送る。
   - `free_ca_mode` codingを使用する規格では、列値の意味がその規格上の `free_ca_mode` 意味に従い、必ずしも物理的なscramble状態を示さないことを明記する。
   - ARIBのように `free_CA_mode` をfree/pay区分へ使用する運用が存在することを、少なくとも誤解を防止できる形で説明する。
2. 公開API名自体の改善、新しい意味の明確な列・alias・deprecation等が必要かをAOSP側で判断してもらうため、Android Issue Trackerへ問題を報告する。

既存public API定数の単純なrenameはAPI互換性へ影響するため、本製品側で解決策を決め打ちしない。まず文書修正パッチを提案し、名称/API設計の変更が必要ならAOSP maintainerの判断へ委ねる。

## 完了条件

次のいずれかを満たした時点で、このnot planned項目を再評価する。

- AOSPへJavadoc修正パッチを提出し、upstreamで扱いが確定した。
- Android Issue Trackerへ報告し、AOSP側で仕様・文書・APIの扱いが確定した。

upstreamの結論に応じて現行投影契約を変更する必要が生じた場合は、まず `ARIB_SI_EPG_TvProvider投影方針.md` を更新し、本future_workは判断理由と再評価履歴だけを追従させる。
