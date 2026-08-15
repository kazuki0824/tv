from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 occurrence, got {count}")
    return text.replace(old, new)


path = Path("tuner_hal/DESIGN_JA.md")
text = path.read_text()
text = replace_once(
    text,
    "| AT-010a | Frontend / Demux / Filter / DVR / Descrambler / LNB open | 能力と容量の検証・資源予約・runtime object準備・registry登録・公開 | registry登録と所有者台帳を確定し、AIDLが要求するobjectおよびout IDを同一応答で返す時点 | 公開前の準備物を逆順に解放する。解放結果を確定できない資源は`CleanupPending`または隔離へ移し、objectもout IDも部分公開しない | 準備中objectと予約済み資源 | 原因別のopenエラーを返し、objectを公開しない | APIごとのAIDL出力形状を維持し、公開失敗後に半登録object、単独のout ID、消費済み容量を残さない |",
    "| AT-010a | Frontend / Demux / Filter / DVR / Descrambler / LNB open | APIごとの公開object / out ID形状を維持し、内部open transactionは「公開transactionのphase・確定点・失敗処理契約」の`root/child open`参照 | objectとout IDを要求するAPIでは両方を同一応答で公開し、内部確定点は`root/child open`参照 | 公開前failure / rollback / cleanup / quarantineは`root/child open`参照とし、callerへobjectまたはout IDを部分公開しない | `root/child open`参照 | 原因別のopenエラーを返し、公開失敗時はobjectを返さない | API固有のcaller-visibleな出力原子性だけを本索引で要求し、予約・prepare・commit・rollback・cleanup semanticsを複製しない |",
    "AT-010a",
)
text = replace_once(
    text,
    "| RL-002 | scan / tune generation | `FrontendHal` | `tune()` / `scan()` | stopTune / stopScan / close / 次generation | コールバック失敗、ワーカー異常 | 古いgenerationの通知を捨て、現generationを失敗状態にする | 古いワーカーが新状態を上書きしない |",
    "| RL-002 | scan / tune generation | `FrontendHal` | `tune()` / `scan()` | stopTune / stopScan / close / 次generation | generation ownerが当該generationの寿命終了を確定した時、または有効性を確認できない時 | 失効または有効性を確認できないgenerationを新generationとして再利用しない。frontend / scanの状態遷移・callback / worker failure semanticsは表19および0-S-3Bのcanonical contract参照 | stale generationから新状態を更新できない |",
    "RL-002",
)
text = replace_once(
    text,
    "| RL-003 | demux generation | `DemuxHal` | demux open / stream boundary reset | demux close | frontend tune boundary、demux fail-closed | demuxを閉鎖側失敗。診断に失敗対象を残す | closed demux向けの後続配送が残らない |",
    "| RL-003 | demux generation | `DemuxHal` | demux open / stream boundary reset | demux close | generation ownerが当該demux generationの寿命終了を確定した時、または有効性を確認できない時 | 失効または有効性を確認できないgenerationを再利用しない。frontend-source / stream boundary / demux failure semanticsは`DemuxFrontendSourceTxn` / `StreamBoundaryTxn`とdemux公開状態契約参照 | stale demux generation向けの配送を新generationへ混入させない |",
    "RL-003",
)
text = replace_once(
    text,
    "| RL-004 | Filter / DVR FMQとdescriptor | 0-S-2で定める各queue owner | object open時にqueueを生成し、descriptor取得時に複製を公開 | object固有cleanupが完了し、queueへの使用権が消滅した時 | queue破損、EventFlag障害、解放結果不明 | 解放を確認できないqueue領域とdescriptor backingを再利用しない。flush / token / drain semanticsは`FilterProducerDrainGate` / `QueueEpochProtocol` / `QueueCleanupTxn`参照 | configureでqueue identityを暗黙置換しない |",
    "| RL-004 | Filter / DVR FMQとdescriptor | 0-S-2で定める各queue owner | object open時にqueueを生成し、descriptor取得時に複製を公開 | object固有cleanupが完了し、queueへの使用権が消滅した時 | queue backing / descriptor control structure / EventFlag objectまたはcontrol blockの構造破損により再利用不能と判定した場合、または解放結果不明 | 解放を確認できないqueue領域とdescriptor backingを再利用しない。payload commit後のEventFlag起床失敗だけでは破棄せず表6/6-Aのqueue runtime契約に従う。flush / token / drain semanticsは`FilterProducerDrainGate` / `QueueEpochProtocol` / `QueueCleanupTxn`参照 | configureでqueue identityを暗黙置換しない |",
    "RL-004",
)
path.write_text(text)

path = Path("開発規則.md")
text = path.read_text()
old = """- `tuner_hal`:
  - `earth_pt1`（Linux Kernel builtin）と `px4_drv`（kazuki0824/px4_drv にある DDK 準拠チューナードライバ）向けの Tuner HAL を実装する。
  - FMQ / dma_heap との界面を除き Rust で実装する。
  - r51 で 対応宣言する `ITuner` / `IFrontend` / `IDemux` / `IFilter` / `IDvr` / `IDescrambler` / `ILnb` 面は、成功 no-op を残さず AOSP 契約どおりに実装する。ただしLNBについては、本書「LNB部分能力公開の製品例外」に定義するproduct-level例外を適用する。
  - `px4_drv` と `earth_pt1` は tune モデル が異なるため、選局候補表の SSOT は本書に置く。製品 scan 候補表の保持と実行時候補生成は TIS が担当し、Tuner HAL 内部では canonical frequency モデル と backend 変換を分離する。
  - 旧 Tuner HAL の参照用ソースとして repository に残す。
  - r50ee5以降、product default の Tuner HAL service には含めない。product package、VINTF manifest、init rc、PRODUCT_PACKAGES、product integration へ旧 `tuner_hal` service を入れてはならない。
  - 設計正本、旧実装の参照、差分確認、単体部品参照には使ってよいが、`android.hardware.tv.tuner.ITuner/default` を登録する実体として扱わない。

- `tuner_hal2`:
  - r50ee5以降の product default Tuner HAL service とする。
  - `tuner_hal` 全体を丸ごとコピーしてはならない。AOSP / ARIB / driver 固有の実ロジック断片だけを選別して取り込む。
  - `android.hardware.tv.tuner.ITuner/default` を登録する実体は `tuner_hal2` だけとする。
  - 旧 `binder_service/src/tuner_hal.rs` の巨大制御層、空構造体、薄い取引型、文字列分類、失敗破棄経路を持ち込んではならない。
"""
new = """- `tuner_hal`:
  - `tuner_hal/DESIGN_JA.md` を Tuner HAL の公開契約・capability・状態遷移・資源寿命の設計正本とし、旧 Tuner HAL 実装ソースは AOSP / ARIB / driver 固有ロジック、差分確認、単体部品の参照用として repository に残す。
  - `px4_drv` と `earth_pt1` は tune モデル が異なるため、選局候補表の SSOT は本書に置く。製品 scan 候補表の保持と実行時候補生成は TIS が担当し、Tuner HAL 内部では canonical frequency モデル と backend 変換を分離する。
  - r50ee5以降、product default の Tuner HAL service には含めない。product package、VINTF manifest、init rc、PRODUCT_PACKAGES、product integration へ旧 `tuner_hal` service を入れてはならない。
  - 設計正本、旧実装の参照、差分確認、単体部品参照には使ってよいが、`android.hardware.tv.tuner.ITuner/default` を登録する実体として扱わない。

- `tuner_hal2`:
  - r50ee5以降の product default Tuner HAL service とし、`earth_pt1`（Linux Kernel builtin）と `px4_drv`（kazuki0824/px4_drv にある DDK 準拠チューナードライバ）向けの Tuner HAL 実装責務を一意に持つ。
  - FMQ / dma_heap との界面を除き Rust で実装する。
  - r51 で対応宣言する `ITuner` / `IFrontend` / `IDemux` / `IFilter` / `IDvr` / `IDescrambler` / `ILnb` 面は、成功 no-op を残さず `tuner_hal/DESIGN_JA.md` のAOSP公開契約に従って実装する。ただしLNBについては、本書「LNB部分能力公開の製品例外」に定義するproduct-level例外を適用する。
  - `tuner_hal` 全体を丸ごとコピーしてはならない。AOSP / ARIB / driver 固有の実ロジック断片だけを選別して取り込む。
  - `android.hardware.tv.tuner.ITuner/default` を登録する実体は `tuner_hal2` だけとする。
  - 旧 `binder_service/src/tuner_hal.rs` の巨大制御層、空構造体、薄い取引型、文字列分類、失敗破棄経路を持ち込んではならない。
"""
text = replace_once(text, old, new, "module responsibility")
path.write_text(text)
