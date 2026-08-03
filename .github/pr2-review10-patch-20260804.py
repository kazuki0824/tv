from pathlib import Path

root = Path(__file__).resolve().parents[1]
path = root / 'tuner_hal/DESIGN_JA.md'
text = path.read_text(encoding='utf-8')

old = '''- セクションフィルター は condition の必要 byte 幅が payload 長を超える場合に match しない。prefix だけ一致した短い payload を match としない。
- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start()`世代内の配送停止条件である。`SectionBits`は最初に一致したsectionを1件配送した後に自動配送を停止する。
- `TableInfo repeat=true`は対応する。AOSP公開条件であるtable idとversionだけで照合し、明示versionではそのversion、`version=-1`では全actual versionを対象として、条件に一致する構造上完全なsectionを継続配送する。callerが指定していないPID、table種別、`table_id_extension`、`last_section_number`、`ProductProfile`の私的一覧で対象を狭めない。
- `TableInfo repeat=false`は、AOSP契約上、callerが指定したtable idとversionに基づくall sectionsを配送してから停止しなければならない。しかしAndroid 14 AIDLには総`table_id_extension`数、対象actual version集合、終了通知がなく、MPEG-TSの`last_section_number`が完結させるのは個々のtable instanceだけである。現行ARIB対象範囲にも、受理可能な全table IDについて未観測instanceの不存在を証明できる単一の規範的最大送出周期はない。このため、汎用的な有限完了を証明できない現行`ProductProfile`では当該組合せを対応済みと表明せず、`configure()`のvalidate段階で`UNAVAILABLE`を返す。既存設定、filter generation、queue、追跡状態を変更しない。
- `TableInfo repeat=false`を、時間窓、最初に完成したcandidate、最初に観測したextension/version、非公開table一覧、再送一巡の推測で成功扱いにしてはならない。将来対応する場合は、公開条件だけから対象全集合と有限終端を証明できる入力構文またはAOSP側の終了情報、および複数extension/versionの適合試験を同一変更で追加する。
- `TableInfo.version`は`-1`または`0..31`だけを受け付ける。`-1`はwildcardであり、runtimeの最初の観測値へ固定しない。範囲外は`INVALID_ARGUMENT`とする。
'''
new = '''- セクションフィルター は condition の必要 byte 幅が payload 長を超える場合に match しない。prefix だけ一致した短い payload を match としない。
- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start()`世代内のone-shot配送停止条件である。`SectionBits`は最初に一致したsectionを1件配送した後に自動配送を停止する。
- `TableInfo`の公開照合条件は、TS filter settingsのPID、table id、versionである。明示versionではそのversionだけを照合し、`version=-1`ではversionを照合条件から外す。callerが指定していないtable種別一覧、送出周期、`ProductProfile`の私的一覧で受理対象を狭めない。
- MPEG-TSの拡張section構文では、規格上の有限な完全集合は1個の具体的table instanceについて`section_number=0..last_section_number`で定義される。本設計では`TableInstanceKey={input_origin_generation, filter_generation, PID, table_id, table_id_extension, actual_version, current_next_indicator}`をinstance identityとし、別extension、別actual version、別current/next、別generationのsectionを同じ完全集合へ混成しない。これらはcallerへ追加の設定条件を課すためではなく、受信sectionを規格上のtable instanceへ分離する内部同一性である。
- `TableInfo repeat=false`は、公開条件に一致して入力順で最初に受理した構造上完全なsectionが属する1個の`TableInstanceKey`をone-shot対象として確定する。`version=-1`は設定上wildcardのまま維持し、選択後のactual versionは異版混成を防ぐinstance identityとしてだけ使用する。拡張sectionでは対象instanceの`0..last_section_number`をsection番号ごとに1件だけ保持し、全番号が揃ってからsection番号順に各sectionを正確に1回配送する。全sectionのFMQ書込みまたはevent登録が確定した後にだけ自動配送を停止する。短形式でversion、extension、section番号を持たないtableは、wildcard設定に一致した最初の完全sectionを1 sectionのinstanceとして配送して停止する。
- one-shot対象を完成前に部分配送しない。`version=-1`で同じextensionのactual versionが完成前に切り替わった場合は、未公開の旧candidateを破棄して新しいcurrent candidateへ切り替える。明示versionでは他versionを無視する。target確定後に別extension/versionが到着しても対象へ混成せず、`repeat=true`では公開条件に一致する全instanceを継続配送する。
- `TableInfo repeat=false`の完了に時間窓、再送一巡、最初に完成したcandidate、非公開table一覧を使用しない。不完全な信号では有限時間で停止することを推測せず、callerの`stop()`、`flush()`、再設定、stream boundaryまで有界メモリーで待機してよい。`flush()`と再設定は未公開candidateを破棄し、旧generationのsectionを新generationへ連結しない。
- SECTION能力閉包は、広告する各section filterについて`tableInfoOneShotBufferBytes = checked_mul(256, maxSupportedSectionBytes)`を予約する。現行TS profileの`maxSupportedSectionBytes`は4096であり、1 filter当たり最大1,048,576 bytesをone-shot candidate用に確保する。FMQ予算とは別台帳とし、この予約を公開filter数分保証できない候補ではSECTION filter数をその閉包内で減らす。広告後の通常入力で容量不足を理由に有効なtable instanceを部分配送または誤完了させない。
- `TableInfo.version`は`-1`または`0..31`だけを受け付ける。`-1`は照合時にversionを無視する指定であり、caller-visibleな設定をruntime観測値へ書き換えない。範囲外は`INVALID_ARGUMENT`とする。
'''
if text.count(old) != 1:
    raise SystemExit(f'TableInfo block match count={text.count(old)}')
text = text.replace(old, new, 1)

old = '''| T-SEC-13 | `SectionBits repeat=false` | one-shot |
| T-SEC-14 | `TableInfo repeat=false` | `UNAVAILABLE`、設定・generation・queue・追跡状態に副作用なし |
| T-SEC-14a | `version=-1`かつ`TableInfo repeat=false` | wildcardを観測値へ固定せず、同じく`UNAVAILABLE`・副作用なし |
| T-SEC-14b | 複数extension/versionが並行する`TableInfo repeat=true` | table id/version条件に一致する全sectionを継続配送し、first-winnerや時間窓で停止しない |
| T-SEC-14c | VTS/product profile | 有限完了を証明する契約と試験が追加されるまで`TableInfo repeat=false`を成功scenarioへ入れない |
| T-SEC-15 | `repeat=true` version更新 | 継続監視 |
'''
new = '''| T-SEC-13 | `SectionBits repeat=false` | 最初の一致sectionを1件配送してone-shot停止 |
| T-SEC-14 | 明示versionの`TableInfo repeat=false`、sectionが順不同 | 最初に選択した`TableInstanceKey`の`0..last_section_number`を全て揃え、section番号順に各1回配送後停止 |
| T-SEC-14a | `version=-1`の`TableInfo repeat=false` | wildcard設定を維持し、選択したactual versionだけでinstanceを完成させ、異版を混成しない |
| T-SEC-14b | 複数extension/versionが並行する`TableInfo repeat=false` | 入力順で最初に受理したmatching instanceをtargetとし、他instanceを混成しない。時間窓またはfirst-completed競争でtargetを変更しない |
| T-SEC-14c | wildcard targetが完成前に同一extensionのcurrent version更新 | 未公開の旧candidateを破棄し、新actual versionのinstanceを新targetとして収集。旧sectionを配送しない |
| T-SEC-14d | 明示version中に他version到着 | 他versionを無視し、要求versionのinstanceだけを待つ |
| T-SEC-14e | short syntax + wildcard + `repeat=false` | 最初の完全sectionを1 section instanceとして1回配送後停止 |
| T-SEC-14f | 最大`last_section_number=255`・各section 4096 bytes | `tableInfoOneShotBufferBytes=1,048,576`以内で全256 sectionを保持し、部分配送・誤完了なし |
| T-SEC-14g | target未完成、`stop()`／`flush()`／再設定／stream boundary | timeoutで誤完了せず、未公開candidateを破棄して世代を分離 |
| T-SEC-14h | 全section完成後のFMQ一時backpressure | 未配送sectionを保持して再試行し、全sectionのcommit前に自動停止またはdropしない |
| T-SEC-14i | 複数extension/versionが並行する`TableInfo repeat=true` | table id/version条件に一致する全instanceのsectionを継続配送する |
| T-SEC-15 | `repeat=true` version更新 | 継続監視 |
'''
if text.count(old) != 1:
    raise SystemExit(f'TableInfo tests match count={text.count(old)}')
text = text.replace(old, new, 1)

old = '| filter main type / FMQ | main type別object数、FMQ byte、callback、assembler、配送worker | demux base、共有worker基盤 | 当該main typeだけを非公開 |'
new = '| filter main type / FMQ | main type別object数、FMQ byte、callback、assembler、配送worker。SECTIONでは公開数分の`tableInfoOneShotBufferBytes`を含む | demux base、共有worker基盤 | 当該main typeだけを非公開 |'
if text.count(old) != 1:
    raise SystemExit(f'closure row match count={text.count(old)}')
text = text.replace(old, new, 1)

old = '''`ProductProfile`に宣言した完全vectorを優先順に検証し、object枠、全依存枠、byte予算を原子的に予約できた最初のvectorだけを確定する。候補の一部列を別候補と混成しない。全vectorを確保できない場合はquery-onlyの縮退状態でserviceを登録し、`getDemuxCaps()`では全demux/filter/DVR個数を0、`getDemuxIds()`は空、各open APIは`UNAVAILABLE`とする。`getFrontendIds()`はprobe結果と独立したroot query最低資源が成立した場合だけ返す。root queryと非対応APIの明示拒否に必要な最小状態も確保できない場合だけBinder serviceを登録しない。変更不能なsnapshotを個数、依存枠、byte予算、受付可否の正本とし、`CleanupPending`または`Quarantined`は解放完了まで使用中と数える。
'''
new = '''サービスオブジェクトの公開個数、FMQ・PES・AV・SECTION one-shot bufferの各byte上限、worker・callback・reaper・cleanup枠は、選択済み`CapabilityClosure`のclaimから導出する。ある閉包候補を予約できない場合は、その閉包と推移的に依存する能力だけを候補から除外し、依存しないfrontend、filter main type、DVR用途を0へ落とさない。`ProductProfile`の優先順は共有資源を競合する閉包候補の選択順にだけ使用し、全能力を含む単一vectorの採否またはquery-only一括縮退へ使用しない。

全閉包の合成後にquery/open、`numDemux`、`filterCaps`、用途別個数、全byte台帳の横断不変条件を検証する。整合したsnapshotを構成できない場合は全仮予約を戻してserviceを登録しないが、AV、PES、SECTION、DVR等の局所閉包不足だけを理由に、整合して残せる無関係な能力を全0にしてserviceを登録する状態は設けない。変更不能なsnapshotを個数、依存枠、byte予算、受付可否の正本とし、`CleanupPending`または`Quarantined`は解放完了まで使用中と数える。
'''
if text.count(old) != 1:
    raise SystemExit(f'complete vector paragraph match count={text.count(old)}')
text = text.replace(old, new, 1)

old = '| FILTER_SECTION | サービス全体 | 8 | `CapabilitySnapshot`の値 | 0 | なし | 呼び出し側指定のFMQ容量はsnapshotの`fmqRuntimeBudgetBytes`から別transactionで予約する。 |'
new = '| FILTER_SECTION | サービス全体 | 8 | `CapabilitySnapshot`の値 | 0 | なし | FMQ容量に加え、各公開filterについて最大256 section×4096 bytesの`tableInfoOneShotBufferBytes`をSECTION閉包から予約する。 |'
if text.count(old) != 1:
    raise SystemExit(f'FILTER_SECTION row match count={text.count(old)}')
text = text.replace(old, new, 1)

path.write_text(text, encoding='utf-8')
