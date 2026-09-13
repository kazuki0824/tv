# r52 AOSP MediaCas / Maleicacid B25 CasPlugin 境界

本書は r52 の **Media CAS service / vendor CasPlugin 境界** の正本である。`future_work/r52/cas_hal_revised_smartcard_yakisoba_plan_v2_b1.md` が定義する backend、lifecycle、key linkage、failure、teardown の意味契約は維持し、Media CAS service と B25 plugin の実装主体だけを本書で固定する。

## 1. service / plugin 構成

`android.hardware.cas.IMediaCasService/default` は AOSP 標準 `MediaCasService` を使用する。Maleicacid は独自 `IMediaCasService/default` service を実装しない。

Maleicacid は AOSP Media CAS plugin ABI に従う loadable shared library を実装する。

```text
TIS / android.media.MediaCas
          |
          | AIDL ICas
          v
AOSP android.hardware.cas.IMediaCasService/default
          |
          | FactoryLoader("createCasFactory")
          v
Maleicacid B25 CasPlugin library
  |- extern "C" createCasFactory()
  |- MaleicacidB25CasFactory : android::CasFactory
  `- MaleicacidB25CasPlugin  : android::CasPlugin
       |- SessionTable
       |- backend binding
       |- YakisobaBackend
       |- SmartCardBackend
       `- Tuner key bridge
```

AOSP `MediaCasService` が plugin library を discovery/load し、`CasFactory` / `CasPlugin` を AIDL `ICas` へ bridge する。Maleicacid plugin library は AIDL `ICas` 自体を実装しない。

plugin library の entry point `createCasFactory()` は C linkage とする。一方、返却する `android::CasFactory` と生成する `android::CasPlugin` は AOSP の C++ virtual interface であるため、Maleicacid の AOSP plugin ABI 境界は C++ で実装する。

## 2. factory / plugin responsibility

`MaleicacidB25CasFactory` は次を所有する。

```text
- B25 CA system ID の support 判定
- B25 plugin descriptor の query
- B25 CasPlugin instance の生成
```

最初の実装では B25 のみを advertise する。B1 は既存の B1 advertise gate を満たした時点で同じ AOSP plugin model に追加できるが、B25 `yakisoba_only` の成立条件にはしない。

`MaleicacidB25CasPlugin` は AOSP `android::CasPlugin` 契約に従い、次を所有する。

```text
- setPrivateData()
- openSession()
- closeSession()
- setSessionPrivateData()
- processEcm()
- processEmm()
- sendEvent() / sendSessionEvent() の unsupported / supported 契約
- provision() / refreshEntitlements() の unsupported 契約
- plugin/session lifecycle
- backend binding
- Tuner key bridge への publish / rotation / revoke
```

AOSP `MediaCasService` が所有するものを Maleicacid 側へ重複実装しない。

```text
- IMediaCasService AIDL service 登録
- enumeratePlugins() の service-level 集約
- isSystemIdSupported() の service-level factory discovery
- createPlugin() の AIDL ICas wrapper 生成
- ICasListener と native CasPlugin callback の bridge
- plugin shared library の discovery / load
```

## 3. ClearKey / descrambler 境界

ClearKey は AOSP 標準 compatibility path のままとし、Maleicacid B25 plugin state と混在させない。

B25 plugin library は Media CAS `DescramblerFactory` を提供しない。B25/B1 の TS packet descramble owner は引き続き Tuner HAL `IDescrambler` とする。

したがって B25 の packet path は次で固定する。

```text
MaleicacidB25CasPlugin
  processEcm()
      |
      | complete MULTI2 material publish / rotation
      v
vendor-internal Tuner key bridge
      |
      | token = MediaCas session ID bytes
      v
tuner_hal2 IDescrambler
      |
      v
TS payload-only MULTI2 descramble
```

key bridge の公開意味契約は `future_work/r52/b25_key_slot_registry_contract.md` を正とする。AOSP `ICas` / `IDescrambler` に vendor 独自 field を追加しない。

## 4. B25 backend ownership

Yakisoba と SmartCard は別 plugin descriptor にせず、1個の B25 `CasPlugin` instance 内部の backend として扱う。

```text
B25Backend
  |- YakisobaBackend
  `- SmartCardBackend
```

backend binding は既存設計どおり plugin instance 単位で一度だけ確定し、release までその plugin の全 session / EMM で共有する。同じ plugin instance の途中で SmartCard と Yakisoba を切り替えない。

product TIS は同一 B25 CA system ID について1個の live MediaCas/CAS plugin を共有し、ECM session と EMM を同じ plugin instance へ配送する。別 client が生成した独立 plugin instance まで service-global backend selector で横断調停しない。

## 5. 最初に成立させる `yakisoba_only`

最初に使用可能にする B25 profile は `yakisoba_only` とする。

`yakisoba_only` の `MaleicacidB25CasPlugin` は生成時から `YakisobaBackend` に bind し、SmartCard probe を行わない。Yakisoba backend failure 時にも同じ plugin instance を SmartCard backend へ切り替えない。

最初の `YakisobaBackend` は plugin library 内の in-process backend とし、Soong で供給される `libyakisoba-cross` の `libyakisoba` C API を使用する。

```text
MaleicacidB25CasPlugin (C++)
        |
        v
YakisobaBackend (C++)
        |
        | bcas_decodeECM() / bcas_decodeEMM()
        v
libyakisoba-cross / libyakisoba
```

Yakisoba backend は libyakisoba の戻り値と key material を AOSP plugin lifecycle / error / key-publish contract へ正規化する。raw key、ECM、EMM、credential を通常 log、TIS、AOSP公開AIDLへ露出しない。

`processEcm()` success は、ECM処理で得た key material を含む complete current MULTI2 material が同じ MediaCas session ID の stable slot へ commit 済みで、Tuner側から解決可能になった時点とする。

`processEmm()` は plugin-wide backend mutation とし、同じ plugin の関連 ECM 処理が半端な EMM 更新 state を観測しない ordering を持つ。

## 6. SmartCard backend

SmartCard は同じ `MaleicacidB25CasPlugin` 内へ後から追加する backend とする。SmartCard追加時も AOSP service、factory、plugin ABI、TIS、Tuner token contract は変更しない。

SmartCard backend は既存設計どおり card probe、初期化、ECM/EMM、card removal、bounded I/O を所有する。`smartcard_only` および `prefer_smartcard_then_yakisoba` は SmartCard backend 実装後に同じ B25 plugin library で成立させる。

## 7. build / Android integration

目標構成では Maleicacid 独自の CAS AIDL service binary、CAS service用 VINTF fragment、CAS service用 init rc を製品経路へ追加しない。AOSP標準 MediaCasService を製品で有効にし、Maleicacid B25 CasPlugin shared library を AOSP MediaCas plugin loader が探索する vendor plugin 配置へ組み込む。

plugin library は `createCasFactory()` を export し、AOSP `media/cas/CasAPI.h` の `android::CasFactory` / `android::CasPlugin` ABI と整合させる。

最初の `yakisoba_only` 構成では plugin library から `libyakisoba` を利用できるよう Soong dependency を設定する。SmartCard component は `yakisoba_only` の build / advertise 条件にしない。

## 8. validation

この service/plugin 境界の完了条件は次とする。

```text
- AOSP MediaCasService が default instance として起動する
- Maleicacid 独自 IMediaCasService service が存在しない
- plugin loader が Maleicacid `createCasFactory()` を発見できる
- `queryPlugins()` に B25 descriptor が1個だけ現れる
- B25 `isSystemIdSupported()` が true
- AOSP `createPlugin(B25)` が Maleicacid CasPlugin を AIDL ICas として返す
- unknown CA system ID は AOSP標準 service semantics のまま unsupported/null
- ClearKey compatibility path を破壊しない
- B25 Media CAS descrambler を追加せず、packet descramble owner は tuner_hal2 のまま
- `yakisoba_only` では SmartCard probe が発生しない
- `yakisoba_only` の ECM/EMM が libyakisoba backendへ到達する
- ECM success後、同じ MediaCas session ID tokenからcomplete current MULTI2 materialを解決できる
- MediaCas session close前の VOID unlink / revoke 契約を維持する
```

## 9. 実装順序

実装時は次の順で成立させる。

```text
1. AOSP CasFactory / CasPlugin library skeleton
2. B25 plugin/session lifecycle
3. YakisobaBackend + libyakisoba integration
4. `yakisoba_only` advertise / ECM / EMM
5. Tuner key bridgeとのend-to-end接続
6. SmartCardBackend
7. `smartcard_only` / `prefer_smartcard_then_yakisoba`
8. B1 plugin support
```

この順序は `yakisoba_only` を最初に使用可能にし、SmartCard未実装をB25 plugin全体の成立条件にしないためのものとする。

## 10. 参照

- AOSP `CasAPI.h`
  - `android::CasFactory`
  - `android::CasPlugin`
  - `extern "C" android::CasFactory *createCasFactory()`
- AOSP CAS AIDL default `MediaCasService`
- AOSP CAS AIDL default `FactoryLoader`
- `future_work/r52/cas_hal_revised_smartcard_yakisoba_plan_v2_b1.md`
- `future_work/r52/b25_key_slot_registry_contract.md`
