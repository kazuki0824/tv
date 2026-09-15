from pathlib import Path

path = Path("開発規則.md")
text = path.read_text()
start = "### r52のMULTI2固定値と動的鍵状態"
end = "### r53 の到達点"
i = text.index(start)
j = text.index(end, i)
replacement = '### r52のMULTI2固定値と動的鍵状態\n\n複数moduleに跨るMULTI2 parameterの所有と共有方針は本節を正本とする。B25とB1は別CAS方式であり、固定parameterを共通値として扱わない。\n\n| 情報 | 所有と使用 | runtime更新 |\n|---|---|---|\n| B25 `system_key` / `cbc_initial_value`（`init_cbc`） | `tuner_hal2`がB25 packet復号に使用するB25専用の製品固定parameter | CAS pluginから配送せず、session登録・ECM更新の対象にしない |\n| B1 `system_key` / `cbc_initial_value`（`init_cbc`） | `tuner_hal2`がB1 packet復号に使用するB1専用の製品固定parameter。B25値を代用しない | CAS pluginから配送せず、session登録・ECM更新の対象にしない |\n| `odd_ks` / `even_ks` | CAS側がECMから成立させ、MediaCas session/tokenに対応付ける動的鍵状態 | 同じtokenの参照先をatomicに更新・失効する |\n\nTISは標準`MediaCas.Session.getSessionId()`と同一bytesのopaque tokenだけをTuner `IDescrambler.setKeyToken()`へ渡す。Tunerはtoken bytesを解析してCA方式を判定しない。vendor内部のtoken解決結果はcurrent Ksまたはそれを使用できる内部鍵資源への参照に加え、その状態を成立させたCAS方式の識別を持ち、Tunerはその方式識別でB25/B1それぞれの固定parameterを選択する。方式識別はvendor内部metadataであり、AOSP公開token形式やTISとのAPIを拡張しない。\n\nB25の`system_key` 32 byteと`cbc_initial_value` 8 byteはB25方式の固定parameterとして扱い、ECM由来KsやYakisoba credentialと同じ秘密性をproduct invariantとして要求しない。正しい方式の値を選ぶこと、vendor image上で意図しない改変を許さないことと、値そのものの秘匿性は別の要件として扱う。Yakisoba credentialの秘密管理はCAS統合側の別契約とし、この固定parameterの扱いから派生させない。\n\nB1の固定parameterはB1方式として独立に接続し、値または取得契約が成立していない状態でB25値へfallbackしてはならない。B1 tokenを解決しても対応するB1固定parameterを利用できない場合は復号可能な鍵状態として扱わず、利用不能として拒否する。\n\nAOSP公開契約はopaque tokenによる`IDescrambler.setKeyToken()`連携であり、動的鍵状態と方式識別を共有する具体的なvendor内部方式は公開契約にしない。特定のtransport・process配置・新規service・公開IPC・公開wire形式を正本で必須化しない。raw Kw / KsをBinderまたはTISへ出さない責務境界を維持する。\n\nsession/tokenと動的鍵状態の一意な対応、atomic update/revoke、stale token・後着結果の排除の詳細は`cas_plugin/DESIGN_JA.md`を正とする。Tuner側のtoken参照・方式別固定parameter選択・packet復号契約は`tuner_hal/DESIGN_JA.md`、その実装ownerは`tuner_hal2/DESIGN_JA.md`を正とし、CAS側のKs更新とTuner側の参照結合を同じ責務にしない。\n'
path.write_text(text[:i] + replacement + "\n" + text[j:])
