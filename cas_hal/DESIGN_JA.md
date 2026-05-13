# CAS HAL 設計判断

## r51 境界

r51 production では、CAS HAL 本体はプレースホルダーであり、対応 CAS system id を広告せず、plugin も返さない。したがって production TIS は実 CAS token を取得できない。実 token が得られない場合、TIS は `setKeyToken()` を呼ばず、スクランブル service を CAS_UNAVAILABLE / video unavailable / 診断へ落とす。

r51 test / diagnostic では、fake CAS、診断注入、Tuner HAL 単体テストに限り、ECM → token → `setKeyToken()` → `addPid()` の接続境界を確認してよい。ただし placeholder token、診断専用 token、fake token を production descrambling success として扱ってはならない。

スクランブル解除成功は CAS HAL 本体実装後、すなわち r52 以降の確認項目とする。

## token 用語

`production token` は r52 以降に CAS HAL 本体が発行する復号用の不透明参照値だけを指す。`fake token` は fake CAS / test 用、`diagnostic token` は診断注入用、`placeholder token` は placeholder CAS 境界確認用であり、いずれも production descrambling success に使ってはならない。TIS は production token が得られた場合だけ Tuner descrambler へ渡す。
