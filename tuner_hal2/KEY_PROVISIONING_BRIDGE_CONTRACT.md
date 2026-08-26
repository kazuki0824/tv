# Tuner key provisioning bridge reliability contract

この文書は `tuner_hal2` 内部の **opaque key token → descrambler key resource** 供給境界について、公開AIDLとは独立した transport reliability / reservation lifetime / secret lifetime の正本である。

Tuner公開契約、`IDescrambler`、key token意味論は `../tuner_hal/DESIGN_JA.md` を正とし、本書はそれらを変更しない。

## 1. 責務境界

`tuner_hal2` は、鍵の供給元が Media CAS / CAS HAL / TEE / test provider / その他のvendor componentのいずれであるかを解釈しない。

Tuner側が知ってよい値は次だけである。

- AOSP `IDescrambler.setKeyToken()` と整合する opaque `key_token`
- `provider_id`
  - 鍵供給adapterが発行するopaque fencing ID
  - Tunerは一致比較以外の意味解釈をしてはならない
- `provider_generation`
  - provider instanceのABA/stale mutationを防ぐopaque generation
- `key_epoch`
  - 同一provider generation内の鍵更新順序
- Tuner自身がpacket descrambleを実行するために必要なalgorithm-specific key resource
  - 現行実装ではMULTI2 system key / CBC IV / even Ks / odd Ks

`tuner_hal2` に次の概念を持ち込んではならない。

- `ca_system_id`
- B25 / B1その他のCA方式名・CA system固定値
- `MediaCas.Session`、CAS plugin、ECM/EMM、SmartCard等のCAS-domain lifecycle
- 上流CAS capabilityやprofileに基づく分岐

上流componentがこれらを必要とする場合は、`tuner_hal2` 外側のintegration adapterが上流identityを `provider_id / provider_generation` へ変換する。Tunerはその変換元を知らない。

`provider_id` はCA system IDの別名として公開・解釈してはならない。複数providerを区別するためのopaque値であり、Tuner側で特定値をallow-listしてはならない。

## 2. command model

内部bridgeは次のcommandだけを持つ。

- `Reserve(key_token, provider_id, provider_generation)`
- `Publish(key_token, provider_id, provider_generation, key_epoch, key_resource)`
- `Revoke(key_token, provider_id, provider_generation)`
- health check用`Ping`

`Reserve`はtoken namespace予約だけを確定し、復号鍵を持たない。`Publish`成功後だけ当該tokenをdescrambler key slotへ解決可能にする。

AOSPのVOID key token `[0x00]` は通常key resourceの識別子として予約してはならない。

## 3. Reserve の確定点と応答喪失

`Reserve(key_token, provider_id, provider_generation)` のTuner registry commit成功を、当該token namespace予約のlinearization pointとする。

response writeはcommitより後であるため、clientから見てtimeout・EOF・接続切断になっても、Tuner側ではReserve済みである ambiguous outcome が成立し得る。

このため次を必須契約とする。

- bridge requestはrequest IDを持つ
- serverはmutation結果をresponse I/Oより先にbounded replay journalへcommitする
- 同一request ID + 同一serialized commandの再送はmutationを再実行せず、保存済みstatusを返す
- 同一request IDを異なるcommandへ再利用した場合はprotocol conflictとして拒否する
- provider clientはresponseを確定できなかったmutationをretryする場合、新しいrequest IDを発行せず、同じrequest IDと同じserialized commandを再送する
- retry回数と各I/O deadlineはboundedとする
- retryを尽くしても結果を確定できない場合、clientは成功を表明しない。Tuner側reservationは下記leaseで回収可能な未publish状態に残す

`Publish` / `Revoke`も同じrequest replay契約を使う。mutationがcommit済みでresponseだけ失われた場合、同じrequest IDの再送で同じstatusを復元する。

## 4. 未publish reservation の bounded lifetime

Reserve済みでまだkey epochをpublishしていないentryは、復号可能tokenではなく lease付きreservation とする。

- live/reserved entry総数には固定上限を設ける。現行defaultは64
- 未publish reservationには有限TTLを設ける。現行defaultは120秒
- 同一token / provider ID / provider generationのReserve retryは同一reservationとして冪等成功し、leaseをrefreshしてよい
- 別provider identityによる同一token Reserveはretryとして扱わず拒否する
- 新しいReserveのcapacity判定前に、TTLを超えた未publish・refcount=0・非revoke entryをreapする
- publish済みentryはTTLでreapしない。publish済みentryは明示revokeとdescrambler ref lifetimeで管理する
- revoke済みでrefcount=0のentryはpersistent tombstoneとして保持せず削除する
- revoke時点でrefが残る場合だけ最終releaseまで隔離する
- reaperはraw/prepared key materialを保持しない。未publish entryには復号鍵を置かない

## 5. secret lifetime

Tunerへ渡されたraw/prepared key materialはTuner自身の復号実装の秘密資源として扱う。

- Debug表示へraw/prepared key bytesを出さない
- wire payload、一時buffer、raw key、prepared keyは不要になった時点でzeroizeする
- replay journalにはkey materialを保持しない
- key schedule/cipher内部で不要な値copyを増やさない
- Tuner側のsecret handlingは鍵の供給元種別と独立して同じ規則を適用する

## 6. owner / test

- protocol framingとreplay journal: `key_provisioning_bridge/src/lib.rs`
- socket server: `aidl_service/src/key_provisioning_bridge_server.rs`
- reservation/key-slot state: `service_runtime/src/descrambler_key_table.rs`
- 上流固有identityからgeneric provider identityへのadapter: `tuner_hal2` の所有外

最低テストは以下を含む。

- protocol crateにCA system ID/B25/B1固定値が存在しない
- 任意のnon-zero `provider_id`を同じ規則で受理する
- mutation commit後にresponseを喪失し、同じrequest IDで再送した場合にmutationを二重適用せず元statusを返す
- 同じrequest IDを別commandへ使うと拒否する
- 同一provider identityのReserve再送がslotを増やさない
- 同一generationでもprovider IDが異なるReserveは同一retryにならない
- TTL超過の未publish reservationが次Reserveのcapacity判定前に回収される
- publish済みentryはTTLで回収されない
- revoke済み・refcount=0 entryがpersistent tombstoneを残さない
- raw/prepared keyがDebugへ露出しない
