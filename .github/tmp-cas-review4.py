from pathlib import Path

cas = Path('cas_plugin/DESIGN_JA.md')
s = cas.read_text(encoding='utf-8')

old = "B1 `processEmm()` はunsupportedとし、stateを変更せずcannot-handle相当statusを返す。r52のB1 ECM-only経路で意味を定義しない `setPrivateData()`、`setSessionPrivateData()`、`sendEvent()`、`sendSessionEvent()`、`provision()`、`refreshEntitlements()` も空successにせずcannot-handle相当statusを返す。`setStatusCallback()`、`closeSession()`、plugin release、stale completion rejection、Tuner key revoke、MediaCas close前のVOID unlinkはB25/B1共通契約に従う。"
new = "B1 `processEmm()` はunsupportedとし、stateを変更せずcannot-handle相当statusを返す。B1のplugin-level `setPrivateData()` はCAT/EMM経路を持たないためunsupportedのままとし、空successにしない。一方 `setSessionPrivateData()` はPROGRAM/ESのCA descriptor private dataを受けるAOSP標準session入力として受理する。入力はCAS scheme-privateなopaque bytesとしてsession-localにcommitし、TISは内容を解釈しない。B1 ECM処理がその内容を必要としない実装でも、未使用であることだけを理由にこの標準入力を拒否しない。更新と `processEcm()` が競合する場合はhalf-committed private dataを観測させない。B1で意味を定義しない `sendEvent()`、`sendSessionEvent()`、`provision()`、`refreshEntitlements()` はstateを変更せずcannot-handle相当statusを返す。`setStatusCallback()`、`closeSession()`、plugin release、stale completion rejection、Tuner key revoke、MediaCas close前のVOID unlinkはB25/B1共通契約に従う。"
assert old in s
s = s.replace(old, new, 1)

old = "- B1で意味を定義しないprivate-data/event/provision/refresh operationが空successせずcannot-handle相当statusを返す"
new = "- PROGRAM/ES CA metadataからB1 sessionへ `setSessionPrivateData()` を成功させ、その後のECM処理まで同じsessionで継続できる\n- B1 plugin-level `setPrivateData()` とB1で意味を定義しないevent/provision/refresh operationが空successせずcannot-handle相当statusを返す"
assert old in s
s = s.replace(old, new, 1)
cas.write_text(s, encoding='utf-8')

tis = Path('tis/DESIGN_JA.md')
t = tis.read_text(encoding='utf-8')
marker = "現行 product では CAS plugin 本体はプレースホルダーのままにする。TIS は Tuner SDK API の filter 経由で PMT/CAT/SDT/ECM/EMM section payload を取得し、PMT/CAT から得た CA_descriptor と SDT 等から得た free_CA_mode / サービス識別子補助情報を arib_si_engine_rs の意味解析結果として受け取る。TIS はcurrent `ServiceSemanticFacts`とcurrent CAS capabilityに基づいて ECM/EMM セクションフィルターと MediaCas/CAS bridgeを型付きAPIで制御し、実keyトークンが得られた場合だけTuner descramblerへ不透明な参照値を渡す。仮実装や診断専用結果は復号成功を意味しないため、`setKeyToken()`へ渡さない。Tuner HALが未接続診断を返した場合も成功扱いにしない。"
addition = marker + "\n\nPROGRAM/ESのCA_descriptor `private_data_byte` はB25/B1をTIS側で解釈せず、対応するMediaCas Sessionの `setPrivateData()` へopaque bytesとして渡す。B1でもこのsession-private-data投入を通常のsession setupとして行い、成功後にECM配送へ進む。CAT由来private dataをMediaCas plugin-level `setPrivateData()`へ渡す経路はEMM対応CA systemだけに限定し、現行のB1では起動しない。CA方式固有のprivate data意味解釈はCAS plugin側の責務とし、TISにB1専用parserやprivate-data抑止分岐を追加しない。"
assert marker in t
t = t.replace(marker, addition, 1)
tis.write_text(t, encoding='utf-8')
