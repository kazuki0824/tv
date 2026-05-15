# ARIB SI エンジン

このディレクトリは TIS が使う TS/SI/EIT 解析ライブラリを Rust で実装する。Kotlin 側には バイナリ section 解析器 を置かない。

主な責務は PAT、PMT、CAT、SDT、NIT、BAT、EIT の解析、ARIB 文字列変換、PMT/CAT の CA_descriptor と SDT 等の free_CA_mode / サービス識別子 補助情報を含む CA情報 / サービスメタデータ意味モデル の生成、TvProvider 反映に必要な snapshot 提供である。raw TS packet demux、PID filter、section assembly、section payload delivery は Tuner HAL の責務であり、arib_si_engine_rs の責務ではない。


## 文字列 decoder の範囲

このライブラリの ARIB 文字列 decoder は、字幕以外の SI/EPG 文字列を対象にする。字幕は TIS 側の字幕 path で `libaribcaption` を使う前提であり、このライブラリは字幕用 decoder の完全実装を 対応宣言しない。`arib_si_engine_rs` は libaribcaption ラッパー を所有しない。

## 公開 API 境界

通常の JNI getter が返す サービス/transport/PMT snapshot は 公開可能スナップショット である。元スナップショット は診断・test の内部確認用に限定する。publish できない サービスの理由は 公開可否診断 getter で確認する。

EIT event は stable identity `onid/tsid/sid/event` と開始時刻を分離して返す。descriptor JSON は診断 API として返し、TIS は必要な範囲を `internal_provider_data` の内部データとして保存する。

ARIB 文字列 decoder は字幕以外の SI/EPG 文字列用であり、字幕は TIS 側の字幕 path で `libaribcaption` により処理する。

### 文字 decoder 固定方針

自前 ARIB 文字列 decoder の完了条件は、mirakc が EPG / サービスモデル 構築で扱う範囲に合わせる。すなわち、字幕本文レンダリングではなく、サービス名、番組名、短形式イベント記述、長形式イベント記述、各種 SI/EPG descriptor の テキストフィールドを安定して文字列化する範囲を対象にする。

この範囲を超える字幕 PES、字幕管理データ、字幕本文、DRCS/外字レンダリング、厳密な組版制御は恒久的に `arib_si_engine_rs` の対象外であり、必要な場合は `libaribcaption` 側の責務とする。未対応 escape / 未対応文字は `panic` ではなく 診断情報と置換文字へ変換する。これは r51 の設計方針として固定する。


## 公開境界の固定

`arib_si_engine_rs` は Android canonical genre を決定しない。旧 `canonicalGenres` event フィールドと indexed JNI getter 群は通常境界に残さず、transaction snapshot と provider-data JNI API に限定する。
