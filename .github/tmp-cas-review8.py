from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if text.count(old) != 1:
        raise RuntimeError(f"{path}: expected exactly one match, got {text.count(old)}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


replace_once(
    "tis/src/com/maleicacid/tvinput/tis/CasController.kt",
    "    private var descramblerClosing = false\n    // AOSP Descrambler は1個のcurrent key slotだけを持つため、現在リンク中のMediaCas systemだけを保持する。\n",
    "    private var descramblerClosing = false\n\n    // AOSP Descrambler は1個のcurrent key slotだけを持つため、現在リンク中のMediaCas systemだけを保持する。\n",
)

replace_once(
    "tis/tests/src/com/maleicacid/tvinput/tis/CasControllerStateTest.kt",
    "        val bridge = RecordingDescrambler().apply {\n            failUnlink = true\n            failClose = true\n        }\n",
    "        val bridge =\n            RecordingDescrambler().apply {\n                failUnlink = true\n                failClose = true\n            }\n",
)
