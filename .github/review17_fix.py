from pathlib import Path

p = Path("開発規則.md")
text = p.read_text(encoding="utf-8")
old = "- sample byte上限は固定値にせず、同一製品profileから生成するHAL `CapabilitySnapshot`のper-event予算と、TISが使用するdecoder入力上限の小さい方へ接続する。対応codecごとの最大正常AUを検証できないprofileでは対応宣言しない。"
new = "- sample byte上限は固定値にせず、TISでは同一`ProductProfile`のcodec、対象decoder/device、最大正常AU、header収集量、reorder depth、allocator上限、実機最悪値を根拠に`TisPlaybackBudgetSnapshot.singleEventLimitBytes`をofflineで独立導出し、AV filter開始前に固定する。TISはHAL内部`CapabilitySnapshot`、`avMaxEventBytes`、`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`をruntime参照・公開・複製しない。\n- HAL側の`ProductProfile.avMaxEventBytes`とTIS側の`TisPlaybackBudgetSnapshot.singleEventLimitBytes`は同一`ProductProfile`の根拠から各層で独立にoffline導出し、対応宣言するcodec/profileの最大正常AUを双方が収容できることをproduct-level invariantとして静的に検証する。いずれかを正の有限値として検証できないprofileはAV対応を宣言せず、私的なTIS→HAL capability経路で補償しない。"
if text.count(old) != 1:
    raise SystemExit(f"expected exactly one old line, found {text.count(old)}")
text = text.replace(old, new)
p.write_text(text, encoding="utf-8")
