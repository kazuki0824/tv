from pathlib import Path

replacements = {
    "tis/INTEGRATION.md": (
        "保留中なら `BootEpgSyncCoordinator` へ同じ開始要求を出す。周期的な監視、新しい永続待ち行列、別の定期実行機構をこの再評価のために追加しない。",
        "保留中なら `JobScheduler` に同じ固定識別子の `BootEpgSyncJobService` を登録する判定へ進む。周期的な監視、新しい永続待ち行列、独自の定期実行機構をこの再評価のために追加しない。",
    ),
    "tis/DESIGN_JA.md": (
        "保留中なら同じ `BootEpgSyncCoordinator` へ開始要求を出す。周期的な監視、新しい永続待ち行列、別の定期実行機構は追加しない。",
        "保留中なら `JobScheduler` に同じ固定識別子の `BootEpgSyncJobService` を登録する判定へ進む。周期的な監視、新しい永続待ち行列、独自の定期実行機構は追加しない。",
    ),
}

for name, (old, new) in replacements.items():
    path = Path(name)
    text = path.read_text()
    if old not in text:
        raise SystemExit(f"{name}: 置換対象が見つからない")
    path.write_text(text.replace(old, new, 1))
