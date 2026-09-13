# CAS HAL 設計判断

## 現行実装

現行本番経路では、`cas_hal` の独自 service はプレースホルダーであり、対応 CAS system id を広告せず、plugin も返さない。B25/B1 の system id も広告対象外である。したがって本番TISは実CASトークンを取得できない。実トークンが得られない場合、TISは `setKeyToken()` を呼ばず、スクランブルサービスを CAS_UNAVAILABLE / video unavailable / 診断へ落とす。

test / 診断では、fake CAS、診断注入、Tuner HAL単体テストに限り、ECM → token → `setKeyToken()` → `addPid()` の接続境界を確認してよい。ただし仮token、診断専用token、疑似tokenを本番経路のスクランブル解除成功として扱ってはならない。

## 目標 service / plugin 境界

r52 の service/plugin 境界は `../future_work/r52/aosp_mediacas_b25_plugin_boundary.md` を正とする。

目標構成では `android.hardware.cas.IMediaCasService/default` はAOSP標準 `MediaCasService` を使用し、Maleicacid独自 `IMediaCasService` serviceは実装しない。

Maleicacidは loadable B25 CasPlugin shared library を実装する。entry point `createCasFactory()` はC linkageとし、plugin ABI境界はAOSP `android::CasFactory` / `android::CasPlugin` C++ interfaceへ合わせる。AOSP MediaCasServiceがこのpluginをロードし、AIDL `ICas` へbridgeする。

YakisobaとSmartCardは同一B25 CasPlugin内部のbackendとする。最初に `yakisoba_only` を成立させ、SmartCard backendは後から同じplugin libraryへ追加する。`yakisoba_only` ではSmartCard probeを行わず、Yakisoba障害時にも同じplugin instanceをSmartCardへ切り替えない。

B25/B1のTS packet descramble ownerはTuner HALのままとする。B25 CasPluginはMedia CAS descramblerを追加せず、ECMで成立したcomplete MULTI2 materialをvendor内部key bridge経由でTuner側stable slotへpublishする。公開tokenはMediaCas session ID bytesを使用し、詳細契約は `../future_work/r52/b25_key_slot_registry_contract.md` を正とする。

## token 用語

`production token` は本番MediaCas session ID bytesをTuner key tokenとして使用する不透明参照値を指す。`fake token` はfake CAS / test用、`diagnostic token` は診断注入用、`placeholder token` は仮実装CAS境界確認用であり、いずれも本番経路のスクランブル解除成功に使ってはならない。TISは本番MediaCas session由来tokenが得られた場合だけTuner descramblerへ渡す。
