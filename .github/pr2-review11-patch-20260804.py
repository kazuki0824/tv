from pathlib import Path
import sys

root = Path(sys.argv[1]).resolve()
path = root / 'tuner_hal/DESIGN_JA.md'
text = path.read_text(encoding='utf-8')

old = '''- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start()`世代内のone-shot配送停止条件である。`SectionBits`は最初に一致したsectionを1件配送した後に自動配送を停止する。
- `TableInfo`の公開照合条件は、TS filter settingsのPID、table id、versionである。明示versionではそのversionだけを照合し、`version=-1`ではversionを照合条件から外す。callerが指定していないtable種別一覧、送出周期、`ProductProfile`の私的一覧で受理対象を狭めない。
- MPEG-TSの拡張section構文では、規格上の有限な完全集合は1個の具体的table instanceについて`section_number=0..last_section_number`で定義される。本設計では`TableInstanceKey={input_origin_generation, filter_generation, PID, table_id, table_id_extension, actual_version, current_next_indicator}`をinstance identityとし、別extension、別actual version、別current/next、別generationのsectionを同じ完全集合へ混成しない。これらはcallerへ追加の設定条件を課すためではなく、受信sectionを規格上のtable instanceへ分離する内部同一性である。
- `TableInfo repeat=false`は、公開条件に一致して入力順で最初に受理した構造上完全なsectionが属する1個の`TableInstanceKey`をone-shot対象として確定する。`version=-1`は設定上wildcardのまま維持し、選択後のactual versionは異版混成を防ぐinstance identityとしてだけ使用する。拡張sectionでは対象instanceの`0..last_section_number`をsection番号ごとに1件だけ保持し、全番号が揃ってからsection番号順に各sectionを正確に1回配送する。全sectionのFMQ書込みまたはevent登録が確定した後にだけ自動配送を停止する。短形式でversion、extension、section番号を持たないtableは、wildcard設定に一致した最初の完全sectionを1 sectionのinstanceとして配送して停止する。
- one-shot対象を完成前に部分配送しない。`version=-1`で同じextensionのactual versionが完成前に切り替わった場合は、未公開の旧candidateを破棄して新しいcurrent candidateへ切り替える。明示versionでは他versionを無視する。target確定後に別extension/versionが到着しても対象へ混成せず、`repeat=true`では公開条件に一致する全instanceを継続配送する。
- `TableInfo repeat=false`の完了に時間窓、再送一巡、最初に完成したcandidate、非公開table一覧を使用しない。不完全な信号では有限時間で停止することを推測せず、callerの`stop()`、`flush()`、再設定、stream boundaryまで有界メモリーで待機してよい。`flush()`と再設定は未公開candidateを破棄し、旧generationのsectionを新generationへ連結しない。
- SECTION能力閉包は、広告する各section filterについて`tableInfoOneShotBufferBytes = checked_mul(256, maxSupportedSectionBytes)`を予約する。現行TS profileの`maxSupportedSectionBytes`は4096であり、1 filter当たり最大1,048,576 bytesをone-shot candidate用に確保する。FMQ予算とは別台帳とし、この予約を公開filter数分保証できない候補ではSECTION filter数をその閉包内で減らす。広告後の通常入力で容量不足を理由に有効なtable instanceを部分配送または誤完了させない。
- `TableInfo.version`は`-1`または`0..31`だけを受け付ける。`-1`は照合時にversionを無視する指定であり、caller-visibleな設定をruntime観測値へ書き換えない。範囲外は`INVALID_ARGUMENT`とする。
'''
new = '''- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start()`世代内のone-shot配送停止条件である。`SectionBits`は最初に一致したsectionを1件配送した後に自動配送を停止する。
- `TableInfo`の公開照合条件は、TS filter settingsのPID、table id、versionである。明示versionではそのversionだけを照合し、`version=-1`では最初のtarget選択時にversionを照合条件から外す。callerが指定していないtable種別一覧、送出周期、`ProductProfile`の私的一覧で受理対象を狭めない。
- Android 14 `SectionSettings`の`repeat=false`がいうall sectionsは、ISO/IEC 13818-1／ARIBの拡張section構文で1個のtableを構成する`section_number=0..last_section_number`である。table idだけではsection番号空間を一意にできず、同じtable idでも`table_id_extension`、actual version、`current_next_indicator`が異なれば別のtable instanceである。本設計では`TableInstanceKey={input_origin_generation, filter_generation, PID, table_id, table_id_extension, actual_version, current_next_indicator}`を内部同一性とし、別instanceの同じsection番号を一つのtableへ混成しない。
- AOSP公開面は`table_id_extension`または全subtable集合の列挙・終端通知を持たない一方、`repeat=false`には有限な停止点が必要である。このため`TableInfo repeat=false`は、公開条件に一致して入力順で最初に受理した構造上完全なsectionが属する1個の`TableInstanceKey`を要求tableとして確定する。これは追加のcaller-visible filter条件ではなく、公開条件が選んだtable種別・versionから、規格上の1個のtableを決定するone-shot解決規則である。`version=-1`はtarget選択まではwildcardのまま維持し、選択後のactual versionは設定値の書換えではなくtable instance identityとして固定する。
- 対象instanceの構造上完全なsectionは、最初の出現順に各section番号を正確に1回だけ逐次配送する。FMQ書込みまたはevent登録が確定した後にだけ対応bitを配送済みbitmapへ立て、重複sectionは再配送しない。`0..last_section_number`の全bitが確定した時点で自動配送を停止する。全payloadをtable完成まで保持せず、section番号順への並べ替えも行わない。短形式でversion、extension、section番号を持たないtableは、公開条件に一致した最初の完全sectionを1 sectionのtableとして配送して停止する。
- target確定後は、別extension、別actual version、別current/nextのsectionを対象へ混成または配送しない。`version=-1`でtarget完成前に別versionが到着してもtargetを先着instanceから切り替えず、明示versionでは他versionを無視する。target内で`last_section_number`が矛盾するsectionはmalformedとして破棄し、誤完了させない。`repeat=true`では公開条件に一致する全instanceを継続配送する。
- `TableInfo repeat=false`の完了に時間窓、再送一巡、最初に完成したcandidate、非公開table一覧を使用しない。不完全なtargetでは有限時間で停止することを推測せず、callerの`stop()`、`flush()`、再設定、stream boundaryまで待機する。`flush()`、再設定、stream boundaryはtarget metadataと配送済みbitmapを破棄し、旧generationのsectionを新generationへ連結しない。
- SECTION能力閉包がone-shot用に確保する追加状態は、1 filter当たり1個の`TableInstanceKey`、`last_section_number`等の固定metadata、および256-bit（32 byte）の配送済みbitmapだけとする。FMQ backpressure中の未確定sectionは既存のsection assembler／配送保留予算で保持し、commit前にbitmapを更新しない。最大256 section分のpayloadを別領域へ常時予約せず、通常のsection組立て・FMQ・配送予算とone-shot追跡状態を二重計上しない。
- `TableInfo.version`は`-1`または`0..31`だけを受け付ける。`-1`はtarget選択時にversionを無視する指定であり、caller-visibleな設定をruntime観測値へ書き換えない。範囲外は`INVALID_ARGUMENT`とする。
'''
if text.count(old) != 1:
    raise SystemExit(f'TableInfo block match count={text.count(old)}')
text = text.replace(old, new, 1)

old = '''| T-SEC-13 | `SectionBits repeat=false` | 最初の一致sectionを1件配送してone-shot停止 |
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
new = '''| T-SEC-13 | `SectionBits repeat=false` | 最初の一致sectionを1件配送してone-shot停止 |
| T-SEC-14 | 明示versionの`TableInfo repeat=false`、sectionが順不同 | 最初に選択した`TableInstanceKey`の各sectionを初出順に1回配送し、`0..last_section_number`の配送済みbitが全て立った後に停止 |
| T-SEC-14a | `version=-1`の`TableInfo repeat=false` | target選択まではwildcardを維持し、先着sectionのactual versionをinstance identityとして固定。設定値は書き換えない |
| T-SEC-14b | 同一table ID/versionで複数extension/current-nextが並行 | 最初の構造上完全なmatching sectionが属するinstanceだけをtargetとし、他instanceの同じsection番号を混成・配送しない |
| T-SEC-14c | wildcard target完成前に別actual version到着 | targetを切り替えず、先着instanceの未配送sectionを待つ。別versionを配送しない |
| T-SEC-14d | target sectionの`last_section_number`不一致 | 不一致sectionをmalformedとして破棄し、bitmapまたは停止判定を進めない |
| T-SEC-14e | short syntax + wildcard + `repeat=false` | 最初の完全sectionを1 section tableとして1回配送後停止 |
| T-SEC-14f | 最大`last_section_number=255` | 256-bit（32 byte）bitmapと固定metadataだけで追跡し、各section payloadは逐次配送してtable全体を保持しない |
| T-SEC-14g | target未完成、`stop()`／`flush()`／再設定／stream boundary | timeoutで誤完了せず、target metadataとbitmapを破棄して世代を分離 |
| T-SEC-14h | 各section配送時のFMQ一時backpressure | 既存の配送保留予算で当該sectionを再試行し、FMQ/event commit前に配送済みbitを立てない |
| T-SEC-14i | 複数extension/versionが並行する`TableInfo repeat=true` | table id/version条件に一致する全instanceのsectionを継続配送する |
| T-SEC-15 | `repeat=true` version更新 | 継続監視 |
'''
if text.count(old) != 1:
    raise SystemExit(f'TableInfo tests match count={text.count(old)}')
text = text.replace(old, new, 1)

replacements = {
    'main type別object数、FMQ byte、callback、assembler、配送worker。SECTIONでは公開数分の`tableInfoOneShotBufferBytes`を含む':
        'main type別object数、FMQ byte、callback、assembler、配送worker。SECTIONでは公開数分の`TableInfoOneShotTracker`（target metadataと256-bit bitmap）を含む',
    'FMQ・PES・AV・SECTION one-shot bufferの各byte上限':
        'FMQ・PES・AVの各byte上限とSECTION one-shot追跡上限',
    'FMQ容量に加え、各公開filterについて最大256 section×4096 bytesの`tableInfoOneShotBufferBytes`をSECTION閉包から予約する。':
        'FMQ容量に加え、各公開filterについて1個のtarget metadataと256-bit（32 byte）の配送済みbitmapをSECTION閉包から予約する。section payloadは逐次配送し、table全体のpayload領域を別途予約しない。',
}
for old_text, new_text in replacements.items():
    if text.count(old_text) != 1:
        raise SystemExit(f'replacement match count={text.count(old_text)}: {old_text}')
    text = text.replace(old_text, new_text, 1)

for stale in (
    'tableInfoOneShotBufferBytes',
    '1,048,576',
    '最大256 section×4096 bytes',
    'one-shot対象を完成前に部分配送しない',
    '全番号が揃ってからsection番号順',
    '未公開の旧candidateを破棄して新しいcurrent candidateへ切り替える',
):
    if stale in text:
        raise SystemExit(f'stale remains: {stale}')

for required in (
    '256-bit（32 byte）の配送済みbitmap',
    '最初の出現順に各section番号を正確に1回だけ逐次配送',
    'FMQ書込みまたはevent登録が確定した後にだけ対応bit',
    '同一table ID/versionで複数extension/current-nextが並行',
    'TableInfoOneShotTracker',
):
    if required not in text:
        raise SystemExit(f'missing required text: {required}')

path.write_text(text, encoding='utf-8')
