# TIS CAS orchestration contract

本書は `tis/DESIGN_JA.md` の「CAS / descrambler 境界」を #57 の production B25/B1 実装について詳細化する。AOSP公開API、CAS HAL、Tuner HALの責務境界を変更しない。矛盾時は `tis/DESIGN_JA.md` とAOSP公開契約を優先する。

## 責務境界

- TISは `MediaCas` / `MediaCas.Session` と Tuner SDK `Descrambler` をオーケストレーションする。
- TISはCAS HAL binder、Tuner HAL binder、generic key provisioning socketを直接呼ばない。
- CAS HALのcard I/O、ECM/EMM意味処理、CW生成、key provisioningはTISへ複製しない。
- Tuner HALはCA system、ECM/EMM、MediaCas sessionの意味を解釈しない。
- ARIB CA descriptorのscope、CA system ID、ECM PID、private dataをCA system IDだけを理由に統合しない。

## MediaCas / Descrambler multiplicity

TISの独立descramble contextは次の組を単位とする。

```text
DescrambleContext =
  service identity
  + CA_system_id
  + ECM PID
  + CA_descriptor private data / scope
  + one MediaCas.Session
  + that session's opaque sessionId/key token
  + one Tuner Descrambler/key slot
  + one or more ES PIDs protected by that same key context
```

同一CA systemの `MediaCas` plugin instanceは複数contextで共有してよい。ただし異なるECM PID、descriptor private data、または独立key contextを同一 `MediaCas.Session` へ押し込まない。`MediaCas.Session.setPrivateData()`を別bindingの値で上書きしてcontextを兼用しない。

同一key contextで保護される複数video/audio PIDは一つのDescramblerへ `addPid()` してよい。独立token/key slotを必要とするcontextは別Descramblerを使用する。TISは「1 CA system = 1 Session/Descrambler」または「1 ES = 1 Descrambler」を固定規則にしない。

Program-level CA descriptorは、そのdescriptorが適用されるESへ展開した後、同一service / CA system / ECM PID / private dataのESを一つのcontextへ束ねる。ES-level CA descriptorが別ECM/private dataを持つ場合は独立contextとする。

## Filter plan ownership

B25/B1固有のECM/EMM実行可否は `CasController` が一度だけ決定し、その `UpdateResult.ecmPids` / `UpdateResult.emmPids` をTuner section filterの唯一のCAS filter planとする。LiveSessionはraw CA metadataから別途ECM/EMM PID集合を再構成しない。

- B25: ECM + EMM
- B1: ECM only
- B1 CAT/EMM metadataはSI事実として保持してよいが、EMM filterをopenせず `MediaCas.processEmm()`も呼ばない。

## Key readiness / playback gate

scrambled serviceのAV playback開始条件は、current generationで必要な全DescrambleContextが `READY` であることとする。

```text
CLEAR            CA descramble contextなし
WAITING_FOR_KEY  session/contextは成立したがtoken/key link未完了
READY            必要な全contextでECM成功、token link、PID linkが成功
ERROR            plugin/session/private-data/token/descrambler等のblocking failure
CLOSED           teardown済み
```

`WAITING_FOR_KEY` をclear playback成功として扱わない。ECM成功後、同じsession ID bytesを `Descrambler.setKeyToken()`へ渡し、必要PIDの `addPid()` が全て成功したcontextだけをREADYにする。通常の後続ECM/CW rotationではMediaCas/CAS HALが同一key slotを更新するため、既にlink済みcontextについてAV pipelineをECMごとに再生成しない。

LiveSessionは `READY` への遷移をsection ingest後のcurrent-state再評価で観測し、その時点で通常の `maybeStartPlayback()` gateへ進む。旧r51 placeholderを理由にscrambled serviceを無条件停止しない。

## Teardown ordering

retune、clear service、CA descriptor/context消滅、CAS fatal failure、session releaseでは、対象contextをcurrent routing mapから先に除去してstale ECM callbackを遮断した後、各contextを次の順でbest-effort teardownする。

1. contextをcurrent ECM/PID routingから除外する。
2. `Descrambler.removePid()` で当該contextのPIDをすべて解除する。
3. keyがlink済みなら `Descrambler.setKeyToken(Tuner.VOID_KEYTOKEN)` でcurrent MediaCas-derived keyをunlinkする。
4. `Descrambler.close()`。
5. `MediaCas.Session.close()`。
6. 同じCA systemで必要なsession/EMM処理がなくなった場合だけ `MediaCas.close()`。

途中のcleanup失敗で後続cleanupを中止しない。TISはvendor-private key registryへ直接Revokeを送らない。MediaCas session closeに伴うprovider-side revokeはCAS HAL/key provisioning側の責務とする。

## CasController executor寿命

`CasController` のmutableなCAS orchestration状態は、専用single-thread executorを直列化境界とする。executor threadの判定はthread nameではなく生成した`Thread` instanceのidentityで行い、同名の別threadを内部executorとして扱わない。

`close()` は同executor上でcurrent contextをrouting mapから除外し、上記teardown orderingに従うcleanup、plugin/descrambler解放、`CLOSED`診断確定までを直列化した後にexecutorをshutdownする。`close()` は冪等であり、shutdown済みexecutorへ再度workを投入しない。

executor shutdown後のmutation入口は新規workを受理せず失敗させる。一方、終了状態を観測するread-only queryはshutdown済みexecutorへのtask投入を必要とせず、`lastDiagnostic()` は確定済み`CLOSED`診断、`currentReadiness()` は`CLOSED`を返せるようにする。

## Generation fencing

- contextはcurrent Tuner/demux generationに属する。
- retune/clear/releaseで旧contextをrouting mapから除外した後に届いたECM/EMM callbackを新generationへ適用しない。
- 旧MediaCas session ID/tokenを新contextへ再利用しない。
- READY/ERROR判定はcurrent context集合だけから算出する。

## 最低試験

- 同一CA system・同一ECM/private dataの複数PIDが一つのcontext/Descramblerを共有する。
- 同一CA systemでも異なるECMまたはprivate dataは別MediaCas.Session/Descramblerになる。
- 一方のcontextへの `setKeyToken()` が他contextのkey linkを置換しない。
- B1 CAT metadataからEMM filter planが生成されない。
- scrambled serviceはECM前にWAITING、必要contextのECM成功後にREADYになる。
- teardownでPID remove → VOID key unlink → Descrambler close → MediaCas.Session closeの順序になる。
- `close()` は同じinstanceへ2回呼んでも安全で、終了後の`lastDiagnostic()` / `currentReadiness()` は`CLOSED`を返し、新規mutation workは拒否される。
- retune/clear後のstale ECMが旧token/PIDを新generationへ適用しない。
