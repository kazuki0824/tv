from pathlib import Path
import re


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


def regex_replace_once(path: str, pattern: str, repl: str) -> None:
    p = Path(path)
    text = p.read_text()
    new, count = re.subn(pattern, repl, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"{path}: regex anchor count={count}: {pattern[:120]!r}")
    p.write_text(new)


# 1) CODE_CONVENTION §2: production #[path] is prohibited. Preserve module
# semantics and move the physical files under boot/, which is already their
# logical module parent.
for name in ("demux_filter_dvr_ops.rs", "packet_ops.rs"):
    src = Path("tuner_hal2/service_runtime/src") / name
    dst = Path("tuner_hal2/service_runtime/src/boot") / name
    if src.exists() and not dst.exists():
        src.rename(dst)
    elif not dst.exists():
        raise SystemExit(f"missing module source {src}")

replace_once(
    "tuner_hal2/service_runtime/src/boot.rs",
    '#[path = "demux_filter_dvr_ops.rs"]\nmod demux_filter_dvr_ops;',
    'mod demux_filter_dvr_ops;',
)
replace_once(
    "tuner_hal2/service_runtime/src/boot.rs",
    '#[path = "packet_ops.rs"]\nmod packet_ops;',
    'mod packet_ops;',
)

bp = Path("tuner_hal2/Android.bp")
text = bp.read_text()
text = text.replace(
    '"service_runtime/src/demux_filter_dvr_ops.rs"',
    '"service_runtime/src/boot/demux_filter_dvr_ops.rs"',
)
text = text.replace(
    '"service_runtime/src/packet_ops.rs"',
    '"service_runtime/src/boot/packet_ops.rs"',
)
bp.write_text(text)

# 2) WorkerRuntime: keep one persistent generic lifecycle owner, but do not
# generalize frontend-specific stop-ticket polling into a second framework.
p = Path("tuner_hal2/DESIGN_JA.md")
text = p.read_text()
text = text.replace(
    '`WorkerRuntime`がgeneric worker lifecycleの唯一のcanonical A state ownerであり、`WorkerHandle`は同ownerに従属するopaqueなtyped handle / authority表現',
    '`WorkerRuntime`がgeneric worker lifecycleの**呼出し越しpersistent state**（worker generation、stop/wake/join authority、bounded reaper queue、pending registry、supervisor active/reaping registry）の唯一のcanonical A state ownerであり、`WorkerHandle`は同ownerに従属するopaqueなtyped handle / authority表現。domain固有stop ticket群のpoll/wait、domain completion/deadline actionなど、1件のWorkerRuntime-managed job実行中だけ存在して外部呼出し越しの別registry/queue/retry正本を形成しないcall-local進行状態はdomain typed jobに保持してよい',
)
text = text.replace(
    '汎用の停止・起床・終了待ち・回収処理・再試行機構のcanonical state ownerは`WorkerRuntime`であり、`WorkerHandle`は従属する物理要素',
    '汎用の停止・起床authorityと、呼出し越しに残るreaper queue / pending registry / active-reaping registryのcanonical state ownerは`WorkerRuntime`であり、`WorkerHandle`は従属する物理要素。frontend固有stop ticketのpoll/wait loopとcompletion/deadline actionはWorkerRuntime-managed job内のcall-local手順であり、独立queue・pending map・retry scheduler・generationを所有しない限り第二A ownerとは扱わない',
)
p.write_text(text)

# 3) TS/UNDEFINED: remove the repo-only probe-only restriction. AOSP/VTS
# requires the advertised TS->TS linkage closure but does not define UNDEFINED
# as a probe-only semantic that must be rejected for normal source linkage.
p = Path("tuner_hal/DESIGN_JA.md")
text = p.read_text()
old_sentence = 'TS `UNDEFINED`同士の接続は`linkCaps`検査用endpointとして別に成功させるが、通常のdata producerへ昇格させない。'
if old_sentence not in text:
    raise SystemExit("probe-only UNDEFINED SSOT sentence missing")
text = text.replace(
    old_sentence,
    'TS `UNDEFINED`はVTSが`linkCaps`の広告closureを構成するために使用するAIDL定義済みsubtypeであり、TS→TSを広告する場合は同一demuxのTS packet source relationとして成功させる。内部packet mechanicsは`TS`/raw sourceと共有してよい。AOSP/VTSが定義しないprobe-only制約は追加せず、非広告main-type pair、加工済みSection/PES/AV/Record outputのTS source化は従来どおり`UNAVAILABLE`とする。',
)
p.write_text(text)

# 4) Section FMQ overflow: align SSOT with AOSP OVERFLOW semantics. Newly
# filtered data may be discarded when the filter buffer is full; repeat=false
# bookkeeping must remain uncommitted so a later broadcast repetition remains
# eligible. Do not introduce a second payload queue behind the FMQ.
replace_once(
    "tuner_hal/DESIGN_JA.md",
    '| T-SEC-14h | 各section配送時のFMQ一時backpressure | 既存の配送保留予算で当該sectionを再試行し、FMQ/event commit前に配送済みbitを立てない |',
    '| T-SEC-14h | 各section配送時のFMQ overflow/backpressure | AOSP `DemuxFilterStatus::OVERFLOW` の契約どおり、filter bufferがfullでcommitできない新規sectionは内部第二queueへ複製・再試行せず破棄して`OVERFLOW`を通知する。FMQ payload/event commitが成功するまでは配送済みbitを立てないため、`repeat=false` targetも未達のまま維持し、放送入力として後続に再到来した同一sectionは通常のmatching対象として再び受理できる |',
)

# 5) Child-open: the former SSOT over-specified a cross-layer physically atomic
# commit. AOSP observes only open success/failure. Keep runtime Prepared until
# Binder object exists; a private callback-store stage may precede Live publish
# because it is not externally reachable, and must be compensated before an
# error is returned if Live publish fails.
replace_once(
    "tuner_hal/DESIGN_JA.md",
    '| root/child open | 公開ID・能力確認 → 全使用権仮予約 → runtime登録準備 → Binder object準備 → 一括commit | objectとruntime登録を同時公開し、途中失敗は全仮予約・artifactを逆順解放 |',
    '| root/child open | 公開ID・能力確認 → 全使用権仮予約 → runtime/object-tableを`Prepared`で登録 → callback/Binder artifact準備 → Binder object準備 → private callback artifact stage → runtime `Live` publish | **AIDLから観測可能な公開確定点はruntime/object-tableの`Live` publishだけ**とする。callback artifactは`Live`前にprivate storeへstageしてよいが、child objectはまだcallerへ返さず通常delivery sourceにもならない。後段`Live` publish失敗時はstage済みartifactと全仮予約を逆順cleanupしてerrorを返し、artifactだけを公開成功状態として残さない。AOSP/VINTF/VTSにvendor内部のcallback storeとruntime tableを同一物理mutationでcommitせよという契約は追加しない |',
)

# 6) px4 DEMOD_LOCK: update stale product SSOT to the exact adopted driver ABI.
p = Path("tuner_hal/DESIGN_JA.md")
text = p.read_text()
pattern = r'## px4_drv ロック 方針\n\n.*?\n## px4_drv chardev open / ライブ TS reader 方針'
replacement = '''## px4_drv ロック 方針

px4_drv backendはRF/carrier lockを直接返すuserspace APIを持たないため、`RF_LOCK`をadvertiseしない。一方、product採用driverは `kazuki0824/px4_drv` `feat/android-ddk` commit `90d9c6506389ece3e47cced826326ccd1c6d22e8`（`Add PX4 demod status readbacks (#1)`）に固定し、同commitのUAPIはread-only `PTX_GET_LOCK_STATUS = _IOR(0x8d, 0x0c, __u32)`を持つ。driverはcurrent system未設定時を`-EAGAIN`とし、それ以外は既存`ops->check_lock()`で現在のdemodulator lockを観測し、I/O/device failureを正常unlocked値へ丸めずerrnoで返す。

HALは同ABIを`device/src/px4/abi.rs::PTX_GET_LOCK_STATUS`として固定し、active backend sessionの`observe_signal_state()`から副作用なくcurrent lockを取得する。したがってpx4の`FrontendInfo.statusCaps`には`DEMOD_LOCK`をadvertiseし、`getStatus(DEMOD_LOCK)`およびtune/scan workerのlock/lost-lock/relock判定は同一generationのfresh readbackから導出する。`PTX_GET_CNR`、TS packet到達、過去の`PTX_SET_CHANNEL`成功履歴だけをcurrent `DEMOD_LOCK`の代替にしてはならない。readback I/O failure、`EAGAIN`、古いgenerationは正常な`false`へ捏造せず既存backend failure/pending契約へ接続する。

`PTX_SET_CHANNEL`が選局時に内部`check_lock()`を使用する既存動作は維持するが、その一回の成功履歴は後続current statusの代替にしない。transport health（188-byte境界、sync、TEI、continuity、無受信時間等）も`DEMOD_LOCK`/`RF_LOCK`の真値へ写像しない。採用driver commitを変更する場合は、新commitに同等のread-only lock ABIとfailure分離が存在することをproduct integration証跡として更新するまでpx4 `DEMOD_LOCK` capabilityを維持してはならない。

## px4_drv chardev open / ライブ TS reader 方針'''
new_text, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
if count != 1:
    raise SystemExit(f"px4 lock section replacement count={count}")
p.write_text(new_text)

# Project-level integration invariant must no longer point at the resolved blocker.
p = Path("開発規則.md")
text = p.read_text()
old = 'px4_drv / product-level invariant として、`PTX_SET_CHANNEL` は driver 内部の `ops->check_lock()` が当該要求の demodulator lock 成立を確認した場合だけ成功する。また、現行 userspace ABI には選局後の current demodulator lock を副作用なく読み戻す経路がない。read-only ABI と I/O failure 分離の追加条件は `future_work/r51/px4_demod_lock_status_readback_blocker.md` を参照し、AOSPへの具体的な状態写像は本書で重複定義しない。'
new = 'px4_drv / product-level invariant として、採用driverは `kazuki0824/px4_drv` `feat/android-ddk` commit `90d9c6506389ece3e47cced826326ccd1c6d22e8` を基準とし、`PTX_SET_CHANNEL` の既存lock確認に加えてread-only `PTX_GET_LOCK_STATUS` が current `ops->check_lock()` 結果を副作用なく返し、I/O failureを正常unlockedと分離することを必須とする。AOSPへの具体的な`DEMOD_LOCK`/event写像は `tuner_hal/DESIGN_JA.md` の「px4_drv ロック 方針」を正本とし、本書で重複定義しない。'
if old not in text:
    raise SystemExit("development-rule px4 invariant anchor missing")
p.write_text(text.replace(old, new, 1))

# Pin the exact adopted commit in the integration SSOT.
p = Path("tuner_hal/INTEGRATION.md")
text = p.read_text()
anchor = 'px4 backend で BS `STREAM_ID` を使う product は、対象 kernel driver が px4_drv `feat/android-ddk` 系であり、BS legacy `slot >= 8` reject が無効で、`PTX_SET_CHANNEL.slot` に absolute TSID を渡せることを事前確認する。確認対象は次である。'
repl = 'px4 backend で BS `STREAM_ID` または `DEMOD_LOCK` current readback を使う product は、対象 kernel driver を `kazuki0824/px4_drv` `feat/android-ddk` commit `90d9c6506389ece3e47cced826326ccd1c6d22e8`（`Add PX4 demod status readbacks (#1)`）または、その契約を明示的に引き継いだ検証済みcommitへ固定する。BS legacy `slot >= 8` reject が無効で、`PTX_SET_CHANNEL.slot` に absolute TSID を渡せること、および `PTX_GET_LOCK_STATUS` がread-only current demod lock ABIとして存在することを事前確認する。確認対象は次である。'
if anchor not in text:
    raise SystemExit("integration px4 anchor missing")
text = text.replace(anchor, repl, 1)
text = text.replace(
    '- HAL 側で TSID -> relative slot 変換表を持たず、absolute TSID をそのまま slot に渡すこと\n',
    '- HAL 側で TSID -> relative slot 変換表を持たず、absolute TSID をそのまま slot に渡すこと\n- `include/ptx_ioctl.h` に `PTX_GET_LOCK_STATUS _IOR(0x8d, 0x0c, __u32)` が存在し、driver実装がcurrent `ops->check_lock()`結果を返すこと\n- HAL `device/src/px4/abi.rs` と backend `observe_signal_state()` が同ABIを使用し、過去のtune成功/CNRをcurrent lockへ代用しないこと\n',
    1,
)
p.write_text(text)

# The blocker conditions are now fulfilled by the pinned driver and HAL path.
blocker = Path("future_work/r51/px4_demod_lock_status_readback_blocker.md")
if blocker.exists():
    blocker.unlink()
