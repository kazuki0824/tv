# ARIB SI エンジン

このディレクトリは TIS が使う TS/SI/EIT 解析ライブラリを Rust で実装する。Kotlin 側には バイナリ section 解析器 を置かない。

主な責務は PAT、PMT、CAT、SDT、NIT、BAT、EIT の解析、ARIB 文字列変換、PMT/CAT の CA_descriptor と SDT 等の free_CA_mode / サービス識別子 補助情報を含む CA情報 / サービスメタデータ意味モデル の生成、TvProvider 反映に必要な snapshot 提供である。raw TS packet demux、PID filter、section assembly、section payload delivery は Tuner HAL の責務であり、arib_si_engine_rs の責務ではない。


## 文字列 decoder の範囲

ARIB 文字列 decoder の適用範囲、完了条件、未対応文字・escape の扱い、字幕との責務境界は `arib_si_engine_rs/DESIGN_JA.md` を正とする。README では同じ設計本文を再定義しない。

このライブラリは、字幕以外の SI/EPG 文字列を安定して文字列化し、TIS が TvProvider へ投影できる構造を返す。字幕本文、字幕管理データ、外字・DRCSを含む字幕表示処理は TIS 側の `libaribcaption` 経路の責務である。

## 公開 API 境界

通常の JNI getter が返す サービス/transport/PMT snapshot は 公開可能スナップショット である。元スナップショット は診断・test の内部確認用に限定する。publish できない サービスの理由は 公開可否診断 getter で確認する。

EIT event は stable identity `onid/tsid/sid/event` と開始時刻を分離して返す。descriptor JSON は診断 API として返し、TIS は必要な範囲を `internal_provider_data` の内部データとして保存する。


## 公開境界の固定

`arib_si_engine_rs` は Android canonical genre を決定しない。旧 `canonicalGenres` event フィールドと indexed JNI getter 群は通常境界に残さず、transaction snapshot と provider-data JNI API に限定する。
