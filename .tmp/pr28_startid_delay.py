from pathlib import Path
p = Path('tuner_hal/DESIGN_JA.md')
s = p.read_text()
anchor = '## フィルタ状態破棄境界と遅延通知方針'
if anchor not in s:
    raise SystemExit('delay section anchor missing')
block = '''## Filter event startId / FilterDelayHint 契約

### `DemuxFilterEvent.startId`

settingsを変更する有効な`configure()`は、commit後の`filter_delivery_generation`に対応するpending startIdをprepareする。同じsettingsの冪等`configure()`では新しいstartIdを発行しない。Filterを再startした後、最初のevent callbackはstartIdだけを含むcallbackとして正確に1回配送し、その後に通常eventを配送する。startId-only callbackに別eventを同梱しない。新規open Filterの最初のstartだけはAOSP予約値0を使用してよく、それ以外は再利用しないpositive idを使用する。stale `filter_delivery_generation`のpending startIdは配送しない。positive idを再利用なしに発行できない場合は既存`filter_delivery_generation` exhaustionの局所failure契約へ従い、新しい独立generation軸を追加しない。

### `FilterDelayHint`

`FilterDelayHint`は`TIME_DELAY_IN_MS`と`DATA_SIZE_DELAY_IN_BYTES`だけを受理する。unknown type、負の`hintValue`、表現不能値は`INVALID_ARGUMENT`で状態不変とする。typeごとに独立したhint値を保持し、0はそのtypeのhintだけをreset、positive値は対応typeの閾値として保持する。media filterは本製品capability契約どおり`UNAVAILABLE`とする。

non-mediaのevent-producing filterではpending `onFilterEvent()` batchを既存Filter delivery ownerが保持し、有効な時間閾値または有効な累積data-size閾値の**いずれか**に達した時点でbatchを配送可能とする。data-sizeはpending batch内eventのdata length合計で評価する。hintはeventを失わせる権限ではなく、stop / flush / close / reconfigure時のpending aggregateは`filter_delivery_generation`でfenceし、旧generationのbatchを新generationとして配送しない。別のscheduler state machineは導入しない。

'''
if '### `DemuxFilterEvent.startId`' not in s:
    s = s.replace(anchor, block + anchor, 1)
else:
    raise SystemExit('startId already present unexpectedly')
p.write_text(s)
final = p.read_text()
for required in ['### `DemuxFilterEvent.startId`','TIME_DELAY_IN_MS','DATA_SIZE_DELAY_IN_BYTES','media filterは本製品capability契約どおり`UNAVAILABLE`']:
    if required not in final:
        raise SystemExit('missing '+required)
