from pathlib import Path

p = Path('cas_plugin/DESIGN_JA.md')
s = p.read_text(encoding='utf-8')


def replace_section(start: str, end: str, body: str) -> None:
    global s
    i = s.index(start)
    j = s.index(end, i)
    s = s[:i] + body.rstrip() + '\n\n' + s[j:]


section3 = '''## 3. B25/B1共通 CasPlugin ABI 契約

`MaleicacidB25CasPlugin` と `MaleicacidB1CasPlugin` は、いずれもAOSP `android::CasPlugin` ABIの同じ公開面を実装する。CA方式に依存しない公開契約を本節に集約し、B25/B1固有差分だけを後続節で規定する。

```text
- setStatusCallback(CasPluginStatusCallback)
- setPrivateData()
- openSession(CasSessionId*)
- openSession(intent, mode, CasSessionId*)
- closeSession()
- setSessionPrivateData()
- processEcm()
- processEmm()
- sendEvent() / sendSessionEvent()
- provision() / refreshEntitlements()
- plugin/session lifecycle
- Tuner key bridge への publish / rotation / revoke
```

採用AOSP plugin ABIに存在しないvendor独自public methodを追加しない。公開面を拡張せず、各CA方式で意味を持たないoperationは空successにせずAOSP既存のcannot-handle相当statusへ写像し、stateを変更しない。invalid/closed session、illegal argument等はAOSP既存statusの意味を保って返す。

`setStatusCallback()` はAOSP `MediaCasService` がplugin生成後に登録するstatus callbackを保持するABI面とする。factoryの `createPlugin()` で受け取った `appData` と組み合わせてstatus eventをservice側へ返す。callback登録前はstatus callbackを発行せず、release確定後はcallbackを発行しない。callback invocationはcommit済みstateだけを通知し、callback中にplugin内部state lockを保持することを要求しない。

引数なしの `openSession(CasSessionId*)` はframeworkのdefault session openであり、B25/B1とも各CA方式のscheme-default MULTI2 sessionを生成する。typed `openSession(intent, mode, ...)` はB25/B1で `LIVE + MULTI2` を通常入力として受理する。AOSP Tuner AIDL VTSのdescrambling profileで当該CA systemを使用する場合は `LIVE + RESERVED` をscheme-default MULTI2への互換入力として受理し、default open / `LIVE + MULTI2` と同じsession coreを生成する。`RESERVED` を別のscrambling algorithmとして広告・実装しない。これ以外の非対応intent/modeはstateを変更せずcannot-handle相当statusを返す。

`processEcm()` の成功条件はB25/B1共通で、対象sessionのcomplete current `Multi2KeyMaterial` が同じMediaCas session ID bytesのstable slotへcommit済みで直ちに解決可能であることとする。commit前にsuccessを返さない。close/releaseとの競合、late completion、revoke、callback orderingは§4および§12〜§13の共通契約に従う。'''
replace_section('## 3. B25 CasPlugin責務', '## 4. plugin / session lifecycle', section3)

section8 = '''## 8. B1

B1の正式対応は `MaleicacidB1CasPlugin` + `B1SmartCardBackend` のECM-only経路とする。Yakisoba backendはB1で使用しない。

B1 pluginは§3の共通AOSP `CasPlugin` ABI契約と§4の共通lifecycle契約に従う。default `openSession(CasSessionId*)` はB1 scheme-default MULTI2 sessionを生成し、typed `openSession(intent, mode, ...)` は `LIVE + MULTI2` を受理する。AOSP Tuner AIDL VTSのdescrambling profileでB1を使用する場合は `LIVE + RESERVED` も同じB1 MULTI2 session coreへの互換入力として受理する。その他の非対応intent/modeはstateを変更せずcannot-handle相当statusを返す。

B1 `processEcm()` のsuccessは§3および§12の共通linearization契約に従い、complete current `Multi2KeyMaterial` が同じMediaCas session IDのstable slotへcommit済みで直ちに解決可能になった時点だけ返す。旧current materialとの新旧混在を許さず、late ECM completionでcurrent materialを巻き戻さない。

B1 `processEmm()` はunsupportedとし、stateを変更せずcannot-handle相当statusを返す。r52のB1 ECM-only経路で意味を定義しない `setPrivateData()`、`setSessionPrivateData()`、`sendEvent()`、`sendSessionEvent()`、`provision()`、`refreshEntitlements()` も空successにせずcannot-handle相当statusを返す。`setStatusCallback()`、`closeSession()`、plugin release、stale completion rejection、Tuner key revoke、MediaCas close前のVOID unlinkはB25/B1共通契約に従う。

B1 plugin advertise gateは次を満たす。

```text
- B1 descriptor / support query / createPlugin(B1)を提供
- default / typed session openを共通契約どおり実装・検証済み
- B1 SmartCard ECM処理を実装・検証済み
- processEcm() success -> stable token / complete current material を検証済み
- processEmm()をunsupportedとして明示
- EMM依存のactivation/control information取得をunsupportedとして明示
- EMM依存の契約更新・権利更新をunsupportedとして明示
- B1でYakisoba backendを選択しない
- generic MULTI2 publish / rotation / revoke / closeを検証済み
```

TISはB1 sessionでEMM filterを起動せず、`MediaCas.processEmm()`を呼ばない。CATにEMM PIDがあってもB1復号開始条件・成功条件にしない。

B1の公開・移植可能な参照実装としては `libaribb1` 系の挙動を一次候補にする。コードを移植・リンクする場合はライセンス条件を実装前に確認する。'''
replace_section('## 8. B1', '## 9. MediaCas session ID / Tuner key token', section8)

section18 = '''## 18. validation

共通CasPluginの最低完了条件は次とする。

```text
- AOSP MediaCasServiceがdefault instanceとして起動する
- Maleicacid独自IMediaCasService serviceが製品経路に存在しない
- effective architectureに応じてplugin `.so` が `/vendor/lib64/mediacas` または `/vendor/lib/mediacas` にinstallされる
- AOSP plugin loaderがその探索directoryからMaleicacid createCasFactory()を発見する
- CasFactoryのlegacy/Ext両createPlugin()がB25/B1とも対応するplugin coreを生成できる
- AOSP MediaCasServiceがExt callback版createPlugin()後にsetStatusCallback()を登録できる
- release確定後にplugin status/event callbackを発行しない
- close/release後のlate resultがkey stateを復活させない
- MediaCas close前のVOID unlink / revoke契約を満たす
- ClearKey compatibility pathを破壊しない
- B25/B1 Media CAS descramblerを追加しない
- packet descramble ownerがTuner HALのままである
- raw key / Kw / Ks / credentialを通常log、TIS、公開AIDLへ露出しない
```

B25 `yakisoba_only` の最低完了条件は次とする。

```text
- B25 descriptorが1個だけ列挙される
- B25 system ID support queryがtrue
- createPlugin(B25)がMaleicacidB25CasPluginをAIDL ICasとして返す
- default openSessionがB25 scheme-default MULTI2 sessionを生成できる
- typed `LIVE + MULTI2` が同じB25 session semanticsを生成できる
- B25をTuner VTS descrambling profileに使う場合、typed `LIVE + RESERVED` がscheme-default MULTI2互換入力として成功する
- yakisoba_onlyではSmartCard probeが発生しない
- ECM/EMMがYakisoba backendへ到達する
- processEcm() success後に同じMediaCas session ID tokenからcomplete current materialを解決できる
- 後続ECMで同じstable slotのmaterialをatomic更新できる
- ECM/EMMは標準processEcm/processEmm入力としてのみ公開AIDLを通し、通常log・別AIDL・診断dump等へ不要に再公開しない
```

B1 ECM-onlyの最低完了条件は次とする。

```text
- B1 descriptorが列挙される
- B1 system ID support queryがtrue
- createPlugin(B1)がMaleicacidB1CasPluginをAIDL ICasとして返す
- default openSessionがB1 scheme-default MULTI2 sessionを生成できる
- typed `LIVE + MULTI2` が同じB1 session semanticsを生成できる
- B1をTuner VTS descrambling profileに使う場合、typed `LIVE + RESERVED` がscheme-default MULTI2互換入力として成功する
- B1でYakisoba backendを選択しない
- B1 processEcm() success後に同じMediaCas session ID tokenからcomplete current materialを解決できる
- B1 processEmm() がstateを変更せずunsupported/cannot-handle相当statusを返す
- B1で意味を定義しないprivate-data/event/provision/refresh operationが空successせずcannot-handle相当statusを返す
- B1 session close/releaseで新規key resolveを遮断し、revoke後のlate ECM結果がkey stateを復活させない
- MediaCas close前のVOID unlink / revoke契約を満たす
```
'''
replace_section('## 18. validation', '## 19. 実装順序', section18)

s = s.replace(
    'Maleicacid B25 CasPlugin shared libraryを64-bitでは',
    'Maleicacid CAS plugin shared libraryを64-bitでは',
    1,
)

p.write_text(s, encoding='utf-8')
