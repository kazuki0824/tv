from pathlib import Path

# DESIGN_JA.md
design = Path('tuner_hal/DESIGN_JA.md')
d = design.read_text(encoding='utf-8')

old = 'この項目は実装規約であるため、詳細な禁止事項、エラー写像、劣化起動、mutex汚染、ワーカー生成・join 方針は `tuner_hal/CODE_CONVENTION.md` を正とする。本書では Tuner HAL が no-`panic` / 劣化起動 / 閉鎖側失敗 を設計上必須とすることだけを固定する。'
new = 'この項目のうち、禁止構文、低レベル失敗の型付き検出、公開status変換の集約方法、mutex汚染、ワーカー生成・joinの実装規約は`tuner_hal/CODE_CONVENTION.md`を正とする。公開AIDL戻り値、status precedence、次状態、資源寿命、閉鎖側失敗対象は本書だけを正本とし、実装規約側で再定義しない。'
assert d.count(old) == 1, d.count(old)
d = d.replace(old, new)

anchor = '`IFilter`、`IDvr`、`IFrontend`、`IDemux`、`ILnb`、`IDescrambler` の 公開メソッド は、AIDL HAL の契約面として close 後状態を必ず検査する。状態別の戻り値、次状態、維持する内部状態、破棄・無効化する内部状態は、本書の「Tuner HAL 状態遷移表SSOT」を正とする。'
addition = anchor + '\n\n通常のメモリ割り当て、FMQの作成・領域確保、共有メモリまたはdma-bufの割り当てについて、要求を満たす容量を確保できないことが確定した場合は`OUT_OF_MEMORY`へ写像する。`UNKNOWN_ERROR`は、容量不足ではない内部不整合、allocator/backendから原因を確定できない異常、または割り当て結果・副作用を確定できない障害に限定する。既知の容量不足を`UNKNOWN_ERROR`へ丸めず、低レベル実装名やerrnoにより公開結果を変えない。個別APIのlifecycle、入力、未対応、commit後失敗が優先される場合は各状態表のpriorityを正とする。'
assert d.count(anchor) == 1, d.count(anchor)
d = d.replace(anchor, addition)

assert d.count('## LNB 固定 profile') == 1
d = d.replace('## LNB 固定 profile', '## LNB能力と固定給電')

old_fixed = '''ただし、公開`ILnb`対応能力と、固定ディッシュ向けsatellite frontendの内部給電は別能力として扱う。`SupportedDeviceCapabilityCatalog`の検証済み項目が、機器ごとの固定電圧、物理rail owner、適用・読戻しまたはfunctional probe、停止時の安全状態、共有時の互換条件を`FixedDishPowerProfile`として一意に定義し、frontend generation開始前に機器単位のrail leaseを取得して固定電圧を実適用できる場合、そのISDB-S frontendは`aidl_baseline_eligible_lnb_count=0`のまま公開してよい。固定給電はfrontend backend内部の選局前提であり、frameworkから選択・変更できるLNB IDとして列挙しない。tune準備失敗では給電とleaseを巻き戻し、`stopTune()`、frontend `close()`、機器切断では同一railの利用generationが0になった時だけcatalogの安全状態へ戻してleaseを解放する。実状態を確定できない場合は当該railと依存frontendを隔離する。

`FixedDishPowerProfile`が未定義、一致しない、固定電圧の適用を確認できない、または対象受信設備がruntime切替を必要とする場合は、そのsatellite frontendを公開しない。VTS/product profileは、公開LNBを使用しない固定給電経路か、将来の`aidl_baseline_eligible`な公開LNB経路かを排他的に選び、固定給電経路で`IFrontend.setLnb()`成功を要求しない。'''
new_fixed = '''ただし、公開`ILnb`対応能力と、固定ディッシュ向けsatellite frontendの内部給電は別能力として扱う。`SupportedDeviceCapabilityCatalog`の機器項目は、内部給電能力を`Disabled`または`Fixed(voltage)`として保持する。`Fixed(voltage)`を指定する場合は、同じ項目に物理rail owner、適用確認方法（読戻しまたはfunctional probe）、停止時の安全状態、共有互換条件を含める。frontend generation開始前に、既存の「LNB機器の資源規則」に従って機器単位のrail leaseを取得し、検証済み固定電圧を実適用できる場合、そのISDB-S frontendは`aidl_baseline_eligible_lnb_count=0`のまま公開してよい。固定給電はfrontend backend内部の選局前提であり、frameworkから選択・変更できるLNB IDとして列挙せず、固定給電frontendで`IFrontend.setLnb()`成功を要求しない。

内部給電能力が`Disabled`、固定電圧または適用確認条件が不一致、固定電圧の適用を確認できない、または対象受信設備がruntime切替を必要とする場合は、そのsatellite frontendを公開しない。給電、lease、tune準備失敗時の巻き戻し、停止時の安全状態復帰、共有railの参照管理、実状態不明時の隔離は、専用profileや別の状態機械を追加せず、本書の「LNB機器の資源規則」「表7」「表8」「ワーカー終了契約」を適用する。'''
assert d.count(old_fixed) == 1, d.count(old_fixed)
d = d.replace(old_fixed, new_fixed)
d = d.replace('`NO_MEMORY`', '`OUT_OF_MEMORY`')
assert 'FixedDishPowerProfile' not in d
assert 'NO_MEMORY' not in d
design.write_text(d, encoding='utf-8')

# CODE_CONVENTION.md
conv = Path('tuner_hal/CODE_CONVENTION.md')
c = conv.read_text(encoding='utf-8')
start = c.index('## 4. AIDLエラー写像\n')
end = c.index('## 5. 起動時 / 実行時失敗モデル\n', start)
replacement = '''## 4. AIDLエラー変換の集約規約

Tuner HALの公開AIDL戻り値、status precedence、次状態、資源変化、閉鎖側失敗対象は`DESIGN_JA.md`の「Tuner HAL 状態遷移表SSOT」だけを正本とする。本書は具体的な`android.hardware.tv.tuner.Result`値の対応表を持たず、低レベル失敗を正本の分類へ接続する実装規約だけを定める。

```text
- device、FMQ、共有メモリ、dma-buf、callback、workerの低レベル失敗は、原因を保持する型付きdomain errorへ変換する
- 容量不足、未対応、入力不正、lifecycle不正、backend内部障害をgeneric errorへ早期に丸めない
- 公開Binder statusへの最終変換は`binder_service`内の状態補助関数またはエラー変換補助関数へ集約する
- Binder method、worker、backend adapter、個別resource helperで公開Result値を直接選択しない
- helper側の分類が`DESIGN_JA.md`の公開契約と矛盾する場合は`DESIGN_JA.md`を正としてhelperを修正する
```

'''
c = c[:start] + replacement + c[end:]

old_row = '| dma-buf確保失敗 | 確保失敗を `NO_MEMORY` または `UNKNOWN_ERROR` へ写像し、非AV filter 経路へ誤波及させない |'
new_row = '| dma-buf確保失敗 | 容量不足と容量不足ではない内部障害を型付きdomain errorで区別し、公開結果は`DESIGN_JA.md`の該当行へ集約して、非AV filter経路へ誤波及させない |'
assert c.count(old_row) == 1
c = c.replace(old_row, new_row)

repls = {
'対象 device node / frontend が存在しない場合でも、HAL サービス自体は起動する。ただし、存在しない frontend / demux / backend resource を capability として advertise してはならない。該当 resource への open / tune / scan は `UNAVAILABLE` を返す。': '対象 device node / frontend が存在しない場合でも、HAL サービス自体は起動する。ただし、存在しない frontend / demux / backend resource を capability として advertise してはならない。該当resourceへの公開結果は`DESIGN_JA.md`の能力・状態表へ写像する。',
'2. 公開Binderメソッド の エラー写像 が本書と実装 helper で一致している': '2. 公開Binderメソッドの最終status変換が`DESIGN_JA.md`の公開契約と一致し、実装helperに別の公開写像表がない',
'- device node 不在、open 不可、permission 不足は `UNAVAILABLE` とする。device が存在する状態での 実行時ioctl失敗 / TS read 失敗 / pump 失敗 は `UNKNOWN_ERROR` とする。': '- device node不在、open不可、permission不足と、device存在下の実行時ioctl/read/pump失敗を型付きdomain errorで区別し、公開結果は`DESIGN_JA.md`へ集約する。',
'- client不正入力 は `INVALID_ARGUMENT` とする。CS110 stream selector 指定、unknown monitor bit、負値または `default_max` 超過の `setMaxNumberOfFrontends()` は `INVALID_ARGUMENT` に固定する。': '- client入力不正は、CS110 stream selector指定、unknown monitor bit、負値または`default_max`超過の`setMaxNumberOfFrontends()`などの入力分類を保持したtyped validation errorとし、公開結果は`DESIGN_JA.md`へ集約する。',
'Target tuner device が存在しない、または権限・device node・driver probing に失敗する場合は劣化起動 とする。HAL サービス自体は登録するが、存在しない frontend / demux / backend resource を capability として advertise しない。`getFrontendIds()` は実在 probe できた frontend だけを返す。存在しない resource への `openFrontend*`、`tune`、`scan` などの public Binder method は `UNAVAILABLE` または対応する service-specific error を返し、サービス起動を `panic` で中断しない。': 'Target tuner device が存在しない、または権限・device node・driver probing に失敗する場合は劣化起動とする。HALサービス自体は登録するが、存在しないfrontend / demux / backend resourceをcapabilityとしてadvertiseしない。`getFrontendIds()`は実在probeできたfrontendだけを返す。存在しないresourceへの公開結果は`DESIGN_JA.md`の該当状態表へ写像し、サービス起動を`panic`で中断しない。',
'mutex汚染は recover-with-inner ではなく閉鎖側失敗とする。runtime オブジェクトの mutex lock に失敗した場合は操作成功扱いにせず、Binder method では `UNKNOWN_ERROR` / service-specific error、内部 HAL path では `HalError::Internal`、非同期ワーカーでは診断ログと `WorkerExit::RuntimeFailure` 相当へ写像する。対象、次状態、後続APIの戻り値は `DESIGN_JA.md` を正とし、本書では再定義しない。汚染後に破損可能な状態を継続利用しない。': 'mutex汚染はrecover-with-innerではなく閉鎖側失敗とする。runtime objectのmutex lockに失敗した場合は操作成功扱いにせず、内部HAL pathでは型付きinternal failure、非同期workerでは診断ログと`WorkerExit::RuntimeFailure`相当へ写像する。公開結果、対象、次状態、後続APIの戻り値は`DESIGN_JA.md`を正とし、本書では再定義しない。汚染後に破損可能な状態を継続利用しない。',
'Public Binder method の error mapping は、入力不正を `INVALID_ARGUMENT`、未対応機能を `UNAVAILABLE`、状態不整合を `INVALID_STATE`、汚染や内部整合性崩壊を `UNKNOWN_ERROR` または `HalError::Internal` 起点の service-specific error に固定する。存在しないオブジェクトを返却する API では AOSP Tuner HAL の該当契約に従い `NAME_NOT_FOUND` または同等の service-specific not-found error を使う。成功を返す場合は、対象 state mutation または query が汚染なしに完了していなければならない。': 'Public Binder methodの最終error mappingは`DESIGN_JA.md`だけを正本とし、本書では入力不正、未対応、lifecycle不整合、not-found、容量不足、内部障害のtyped domain分類を失わず`binder_service`の単一変換境界へ渡すことだけを固定する。成功を返す場合は、対象state mutationまたはqueryが汚染なしに完了していなければならない。',
'- lifecycle違反、owner不一致、foreign object、closed object は対象APIの `INVALID_STATE` / `INVALID_ARGUMENT` に写像し、backend failureへ昇格させない。': '- lifecycle違反、owner不一致、foreign object、closed objectは対象APIのtyped validation/lifecycle failureとして保持し、backend failureへ昇格させない。公開結果のprecedenceは`DESIGN_JA.md`を正とする。',
}
for old, new in repls.items():
    assert c.count(old) == 1, (old[:80], c.count(old))
    c = c.replace(old, new)

c = c.replace('`NO_MEMORY`', '`OUT_OF_MEMORY`')
assert 'NO_MEMORY' not in c
for token in ('`INVALID_ARGUMENT`','`INVALID_STATE`','`UNAVAILABLE`','`UNKNOWN_ERROR`','`NAME_NOT_FOUND`','`OUT_OF_MEMORY`'):
    assert token not in c, token
conv.write_text(c, encoding='utf-8')
