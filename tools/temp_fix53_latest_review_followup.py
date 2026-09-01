from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


# T-SEC-14h: remove the remaining sentence that contradicted both the AOSP
# OVERFLOW definition and the repository's own drop-new queue-full contract.
replace_once(
    "tuner_hal/DESIGN_JA.md",
    '- SECTION能力閉包がone-shot用に確保する追加状態は、1 filter当たり1個の`TableInstanceKey`、`last_section_number`等の固定metadata、および256-bit（32 byte）の配送済みbitmapだけとする。FMQ backpressure中の未確定sectionは既存のsection assembler／配送保留予算で保持し、commit前にbitmapを更新しない。最大256 section分のpayloadを別領域へ常時予約せず、通常のsection組立て・FMQ・配送予算とone-shot追跡状態を二重計上しない。',
    '- SECTION能力閉包がone-shot用に確保する追加状態は、1 filter当たり1個の`TableInstanceKey`、`last_section_number`等の固定metadata、および256-bit（32 byte）の配送済みbitmapだけとする。FMQ overflow/backpressureでcommitできない新規sectionはAOSP `DemuxFilterStatus::OVERFLOW`のdrop-new意味論に従って破棄し、commit前にbitmapを更新しない。最大256 section分のpayloadを別領域へ常時予約せず、通常のsection組立て・FMQ予算とone-shot追跡状態を二重計上しない。後続放送入力として同一sectionが再到来した場合は、bitmap未確定のため通常のmatching対象として受理できる。',
)

# Physical implementation anchors must follow the CODE_CONVENTION-compliant
# module move. This changes no logical owner or entry point.
p = Path("tuner_hal2/DESIGN_JA.md")
text = p.read_text()
text = text.replace(
    'service_runtime/src/demux_filter_dvr_ops.rs',
    'service_runtime/src/boot/demux_filter_dvr_ops.rs',
)
text = text.replace(
    'service_runtime/src/packet_ops.rs',
    'service_runtime/src/boot/packet_ops.rs',
)
p.write_text(text)
