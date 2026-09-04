# Maleicacid CAS HAL 統合条件

本書はCAS HALのproduct統合、process境界、VINTF、init、SELinux、外部依存、配布条件の正本である。runtimeの状態、API戻り値、path選択、鍵寿命は `DESIGN_JA.md` を正とする。

## partition とservice

production service、vendor key bridge、SmartCard adapter、任意のYakisoba daemonはvendor partitionに閉じる。service instanceはAIDL VINTF version 1の `android.hardware.cas.IMediaCasService/default` とし、実装binary、init rc、VINTF fragment、vendor sepolicyを同じproduct profileで組み込む。

production moduleは `maleicacid.tv.cas_hal-service` である。旧 `maleicacid.tv.cas_hal-stub-service`、旧init rc、旧VINTF fragmentは削除済みであり、別treeから旧moduleを持ち込んでservice instanceを重複登録しない。

production product packageは論理的に次を含む。

```text
- Maleicacid CAS HAL service
- AOSP/VTS ClearKey compatibility plugin/descrambler path
- CAS/Tuner shared key registry bridge
- SmartCard I/O adapterと必要なPC/SC stack
- init rc / AIDL VINTF fragment / vendor sepolicy
- userdebug/engで明示選択した場合だけYakisoba daemon
```

## build profile

path profileはproduct image生成時に `/vendor/etc/maleicacid/cas_capabilities` として固定する。このfileはB25/B1の配布image能力manifestであり、起動後に変更する一般設定またはruntime切替点として扱わない。serviceは起動時に一度だけstrict parseし、欠落、不正、重複entryでは空のMaleicacid product capability snapshotへfail-closedする。このfail-closedはB25/B1だけに適用し、AOSP/VTS互換のClearKey `0xF6D8` capabilityを無効化しない。

許可entryは次のとおりである。

```text
b25-smartcard
b25-smartcard-yakisoba
b25-yakisoba
b1-smartcard
```

B25 entryは最大1個、B1 entryは最大1個とする。各entryは対応するadvertise gateを完了したproductだけが同梱する。ClearKeyはこのprofileへentryを持たず、Maleicacid product capability gateから独立する。

device product側では、gate完了後に管理対象profileをvendor partitionへcopyする。repositoryは未検証B25/B1能力を有効化するdefault profileを配布しない。

```make
PRODUCT_COPY_FILES += \
    device/<vendor>/<product>/cas_capabilities:$(TARGET_COPY_OUT_VENDOR)/etc/maleicacid/cas_capabilities
```

| build | B25 profile | B1 profile | Yakisoba同梱 |
|---|---|---|---|
| `user` / 配布image | `smartcard_only` | `smartcard_only` | しない |
| `userdebug` / `eng` default | `prefer_smartcard_then_yakisoba` | `smartcard_only` | GPL配布条件を満たす構成だけ |
| `userdebug` / `eng` 明示実験 | `yakisoba_only`をB25だけに許可 | `smartcard_only` | 必須 |

profile snapshotはB25/B1 service capabilityと一緒に起動時に確定し、session途中で更新しない。B1に `yakisoba_only` を設定したbuildは構成エラーとする。

product makefileでは `cas_hal/config/product_integration.mk` を継承し、BoardConfigでは `cas_hal/config/BoardConfigVendorSePolicy.mk` をincludeする。CAS service binaryにはAOSPの `hal_cas_default` domainを使い、CAS→Tuner鍵bridgeのinit socketには専用typeを付ける。SmartCard/Yakisoba adapter固有socket typeとpeer domainは採用adapterと同じproduct sepolicyで追加し、一般domainへ接続権を広げない。

## SmartCard統合

card reader device node、USB/CCIDまたはPC/SC daemonへのアクセスは専用vendor domainへ最小権限で許可する。CAS HALの `media` 実行user/groupだけを根拠に広いdevice accessを付与しない。ueventd ownership、SELinux type、service domain transition、binder call先をproductで明示する。

card I/Oにはopen/reset/APDUごとの有限deadlineと取消経路を持たせる。reader/card不在、card無効、非対応card、I/O利用不能、timeoutを同じerrnoへ丸めず、`DESIGN_JA.md` のprobe分類へ写像する。実機gateではB25 card、B1 cardまたは妥当なB1 test vector、card抜去、timeout、再openを確認する。

system keyとCBC初期値のcredentialはvendor secure provisioningから取得する。Android property、TIS、Tuner HAL、通常のworld-readable fileをcredential sourceにしない。採用するsecure store、key rotation、revoke、factory provisioningはdevice product側で固定し、未設定imageではB25/B1 capabilityを広告しない。

### adapter IPC wire contract

CAS serviceはB25 SmartCard adapterへ `/dev/socket/maleicacid_cas_b25_smartcard`、B1 SmartCard adapterへ `/dev/socket/maleicacid_cas_b1_smartcard`、任意のB25 Yakisoba daemonへ `/dev/socket/maleicacid_cas_yakisoba` で接続する。各要求はbig-endianのversion 1 frameで、32 byte header `MCAS | version | operation | system | path | request_id:u64 | session_generation:u64 | session_len:u8 | reserved[3] | payload_len:u32`、続いてsession IDとpayloadを持つ。operationはopen=1、session private data=2、ECM=3、EMM=4、close=5、systemはB25=1/B1=2、pathはSmartCard=1/Yakisoba=2とする。

応答は20 byte header `MCAR | version | status | path | reserved | request_id:u64 | payload_len:u32`とpayloadである。statusは順にok=0、bad-value=1、cannot-handle=2、invalid-state=3、resource-busy=4、no-license=5、license-expired=6、not-provisioned=7、no-card=8、card-mute=9、card-invalid=10、I/O-unavailable=11、timeout=12、unknown=13とする。ECM成功payloadだけが `system_key[32] | cbc_initial_value[8] | even_ks[8] | odd_ks[8]` の56 byteを返し、他の成功応答は空とする。最大frameは4256 byte、I/O deadlineは2秒であり、version、reserved、request ID、path、長さの不一致を成功へ丸めない。

adapterはpeer credentialとSELinux domainを検証し、同一card I/Oを直列化する。各socketのfile type、adapter domain、CAS domainからのconnect permissionは採用adapterのproduct sepolicyで定義する。repository内の基本sepolicyはCAS→Tuner鍵bridgeだけを許可し、未選定adapterへ広い接続権を先置きしない。

## Yakisoba daemon統合

libyakisobaはCAS HALへ静的/動的linkせず、専用vendor daemon processに閉じる。daemonはB25 ECM/EMMだけを受け付け、B1を拒否する。IPC endpointはCAS HAL domainだけが接続でき、peer credential検証、SELinux allowlist、message size上限、request deadline、再起動時generation fenceを持つ。socket/file descriptorをsystem app、TIS、Tuner HAL、shellへ公開しない。

daemonの設定とcredentialはvendor-private locationから最小権限で読み、検索pathやcurrent working directoryへfallbackしない。ECM/EMM、APDU、key material、session ID、tokenをlogcat/tombstone message/dumpへ出さない。daemon crashまたはtimeoutを同一sessionのSmartCard fallbackへ接続しない。

## ライセンスと配布

採用候補の `tsukumijima/libaribb25` repositoryはApache License 2.0を掲示しており、そのB1対応codeを参照・移植・linkする場合は、採用revisionを固定し、Apache-2.0本文、著作権表示、NOTICE条件、改変表示を配布物へ反映する。別のlibaribb1 sourceを採用する場合は、取り込み前にそのexact revisionのlicenseを再確認する。

`tsunoda14/libyakisoba` はGPL-3.0である。binaryまたは改変版をimage/配布物へ含める場合は、GPL本文、著作権表示、対応する完全なソース、改変済みbuild/install情報など採用revisionに適用されるGPLv3の配布条件を満たす。daemon分離はCAS HALとのprocess/権限境界を明確にする設計であり、libyakisoba/daemon側のGPL義務を消す根拠にしない。配布条件を満たせないbuildではYakisoba moduleをproduct graphから除外する。

third-party sourceは検証済みcommitへpinし、branch tipやdownload時点の未固定archiveをrelease入力にしない。Soong license metadataとNOTICE generationをbuild gateに含める。

調達時の一次確認先は次に固定する。実装開始時は採用commitの同じfileを再確認し、URL先の後続変更だけで既存release入力の条件を上書きしない。

- libaribb25/B1 code: <https://github.com/tsukumijima/libaribb25> / <https://github.com/tsukumijima/libaribb25/blob/master/LICENSE>
- libyakisoba: <https://github.com/tsunoda14/libyakisoba> / <https://github.com/tsunoda14/libyakisoba/blob/master/COPYING>
- Android CAS AIDL: <https://android.googlesource.com/platform/hardware/interfaces/+/refs/heads/android14-release/cas/aidl/android/hardware/cas/>

## build・試験・実機gate

production capabilityを広告するimageでは少なくとも次を確認する。

```text
- CAS AIDL service、registry bridge、SmartCard adapterをSoong buildできる。
- AIDL VINTF fragment、init service、SELinux domain、service registrationが一致する。
- ClearKey `0xF6D8` descriptorが常に列挙され、AOSP/VTS compatibility plugin/descrambler pathが成立する。
- unknown system IDはAIDL成功でfalse/nullを返す。
- B25/B1 descriptorとisSystemIdSupported/createPluginが同じMaleicacid product capability snapshotに従う。
- B25/B1のCAS HAL createDescrambler/isDescramblerSupportedは非対応のままである。
- CAS unit/integration test、AIDL VTS、Tuner descrambler結合試験が通る。
- 実cardまたは妥当なvectorでB25 ECM/EMM、B1 ECM-onlyを確認する。
- session ID tokenでTuner registryを解決し、scrambled TSがpayload-onlyで復号される。
- close/release/card抜去/daemon crashでtokenがrevokeされ、stale tokenが再利用されない。
- secret scanでBinder/log/dump/image内の意図しないkey material露出がない。
- third-party license/NOTICE/source-offer成果物を生成できる。
```

Yakisobaを含まないuser buildではdaemon binary、init rc、sepolicy allow、license payloadを残さない。Yakisobaを含むdebug buildではB25 fallback全分類とB1拒否を追加確認する。

production CAS service、CAS/Tuner鍵bridge、host unit test、Soong module定義、init/VINTF fragment、基本sepolicyはrepositoryへ実装済みである。Android/Soong実build、AIDL VTS、採用SmartCard adapterとsecure credentialの結合、実card/放送波確認はこの変更では実行しておらず、それらのgateを完了していないproductはB25/B1 capability profileを同梱してはならない。
