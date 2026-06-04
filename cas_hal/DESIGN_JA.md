# CAS HAL 設計判断

## r51 境界

r51 本番経路では、CAS HAL 本体はプレースホルダーであり、対応 CAS system id を広告せず、plugin も返さない。B25/B1 の system id も広告対象外である。したがって 本番TIS は実 CAS トークン を取得できない。実 トークン が得られない場合、TIS は `setKeyToken()` を呼ばず、スクランブル サービスを CAS_UNAVAILABLE / video unavailable / 診断へ落とす。

r51 test / 診断 では、fake CAS、診断注入、Tuner HAL 単体テストに限り、ECM → トークン → `setKeyToken()` → `addPid()` の接続境界を確認してよい。ただし 仮トークン、診断専用 トークン、疑似トークン を 本番経路のスクランブル解除成功 として扱ってはならない。

スクランブル解除成功は CAS HAL 本体実装後、すなわち r52 以降の確認項目とする。

## トークン 用語

`production token` は r52 以降に CAS HAL 本体が発行する復号用の不透明参照値だけを指す。`fake token` は fake CAS / test 用、`diagnostic token` は診断注入用、`placeholder token` は 仮実装 CAS 境界確認用であり、いずれも 本番経路のスクランブル解除成功 に使ってはならない。TIS は 本番経路 トークン が得られた場合だけ Tuner descrambler へ渡す。
