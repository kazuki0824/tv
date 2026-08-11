from pathlib import Path
import re


def replace_section(path: str, pattern: str, replacement: str) -> None:
    target = Path(path)
    text = target.read_text()
    updated, count = re.subn(pattern, replacement.rstrip(), text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"{path}: 対象章を一意に特定できない")
    target.write_text(updated)


replace_section(
    "tis/DESIGN_JA.md",
    r"### Direct Boot drain / ライブセッション 優先\n.*?(?=\n## TIS コールバック 入力境界と逆圧)",
    """### Direct Boot の保留処理とライブセッションの優先順位

`MaleicacidTvInputService.onCreate()` は Direct Boot の保留処理、起動時の EPG 同期、定期保守を開始しない。`DirectBootEpgPending` を確実に処理するための再開点は、`AndroidManifest.xml` に登録した `BootReceiver` が受ける `ACTION_BOOT_COMPLETED` とする。`ACTION_LOCKED_BOOT_COMPLETED` では `DirectBootEpgPending` の記録だけを行い、TvProvider、Tuner、JNI 経由の解析処理は起動しない。利用者のロック解除までプロセスが生存している場合は、動的に登録した `ACTION_USER_UNLOCKED` の受信処理から同じ保留処理を前倒ししてよいが、保留処理を確実に再開できることは `ACTION_BOOT_COMPLETED` によって保証する。定期保守の実行機構だけに `DirectBootEpgPending` の処理保証を依存させない。

`ACTION_BOOT_COMPLETED` と補助的な `ACTION_USER_UNLOCKED` が重複して到達しても、同じ `inputId` の保留処理を重複して確定してはならない。開始要求を何度受けても同じ結果になるようにし、同期処理を開始できなかった場合、および TvProvider への反映が正常終了する前は `DirectBootEpgPending` を維持する。`DirectBootEpgPending` は、前節で定めた起動時 EPG 同期の反映処理が正常に確定した場合だけ解除する。

起動時の EPG 同期と定期保守を開始できるのは、`activeLiveSessionCount == 0`、`sessionCreationInProgress == false`、`setupScanRunning == false`、`playbackPipelineRunning == false`、`scanManager running == false` をすべて満たす場合だけとする。ライブセッションの作成要求が来た時点でこれらの処理をまだ開始していない場合は開始を見送る。すでに実行中の場合は停止または延期し、ライブ視聴の選局を優先する。
""",
)

replace_section(
    "tis/INTEGRATION.md",
    r"## Direct Boot と boot receiver\n.*?(?=\n## flash 後の確認)",
    """## Direct Boot と起動時の受信処理

TIS は `directBootAware=true` を維持する。`AndroidManifest.xml` の `BootReceiver` は `android:directBootAware=\"true\"` とし、`ACTION_LOCKED_BOOT_COMPLETED` と `ACTION_BOOT_COMPLETED` の双方を受信対象にする。`ACTION_LOCKED_BOOT_COMPLETED` ではデバイス保護領域に `DirectBootEpgPending` だけを記録し、TvProvider、Tuner、JNI 経由の解析処理は起動しない。

`ACTION_BOOT_COMPLETED` では `UserManager.isUserUnlocked()==true` を確認し、`DirectBootEpgPending=true` なら起動時の EPG 同期を開始対象にする。同じ `inputId` に対する開始要求を複数回受けても同じ結果になるようにし、同じ反映処理を重複して確定しない。起動時の EPG 同期を開始できなかった場合、または TvProvider への反映が正常終了する前は `DirectBootEpgPending` を維持し、正常な反映処理の確定後にだけ解除する。

`ACTION_USER_UNLOCKED` は `AndroidManifest.xml` に登録しない。プロセスが利用者のロック解除まで生存している場合は、動的に登録した受信処理から同じ保留処理の開始を前倒ししてよい。ただし、`DirectBootEpgPending` を確実に再開する正規の入口は `ACTION_BOOT_COMPLETED` とし、動的な `ACTION_USER_UNLOCKED` の受信や定期保守の実行機構だけに再開保証を依存させない。`MaleicacidTvInputService.onCreate()` は Direct Boot の保留処理、起動時の EPG 同期、定期保守を開始してはならない。

起動時の EPG 同期と定期保守を開始できるのは、ライブセッション、セッション作成中、設定用の走査、再生処理、走査管理処理がすべて存在しない場合だけとする。ライブセッションの作成要求が来た時点でこれらの処理をまだ開始していない場合は開始を見送る。すでに実行中の場合は停止または延期し、ライブ視聴の選局を優先する。
""",
)
