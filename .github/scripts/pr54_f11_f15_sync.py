from pathlib import Path
import re


def replace_one(text: str, pattern: str, replacement: str, label: str) -> str:
    result, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"{label}を一意に置換できません: {count}")
    return result


design_path = Path("tis/DESIGN_JA.md")
design = design_path.read_text()

design = replace_one(
    design,
    r"TISの物理候補表は製品scan実装データのSSOTであり、.*?driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。",
    """TISの物理候補表は製品scan実装データのSSOTであり、`開発規則.md`の規範値に従うRF候補を唯一保持する。BS setup/rescanは物理RFごとにstream selector未指定の`IsdbsFrontendSettings`でAOSP `Tuner.scan()`を実行し、`ScanCallback.onInputStreamIdsReported()`で得たcurrent stream IDをtyped `STREAM_ID` explicit tune candidateへ変換する。dynamic stream-ID discoveryが正常完了して1件以上のIDを報告した場合は、その報告IDだけを候補にする。scan開始後の失敗、timeout、または正常完了してもstream IDが0件の場合はfail-closedとし、versioned TSID表を代入しない。一方、frontend/HALがdynamic stream-ID discoveryという能力自体を提供しないことをframeworkの`Tuner.RESULT_UNAVAILABLE`として明示した場合に限り、`開発規則.md`のversioned BS TSID表を当該RFのexplicit `STREAM_ID` tune候補seedとして使用してよい。TISはbackend名、driver名、HAL内部のeffective capabilityを取得・推測してこの分岐を行わず、frameworkから返るtyped resultだけを判断入力にする。versioned TSID表はchannel登録事実ではなく選局候補に限定し、各候補を実際にtuneした後、PAT/NIT/SDT actualからONID/TSID/SIDとcurrent transportを確認できたserviceだけを登録・公開する。driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。""",
    "F15設計段落",
)

design = replace_one(
    design,
    r"- ARIB exceptional ratingのpolicy ownerはLive TV Appとする。.*?TISはraw値から独自policyを実装せず\s*`TvInputManager\.isRatingBlocked\(\)`\s*の結果だけに従う。",
    """- ARIB exceptional ratingのpolicy ownerはLive TV Appとする。rating定義は独立`AribContentRatings` APKがTIF標準providerとして公開し、System TV App本体へ直接patchを当てない。Android 15 / LineageOS 22.1系の既存`ContentRatingLevelPolicy`はTIF rating-provider XMLの`contentAgeHint`をpreset policyの入力として使い、`HIGH`は6以上、`MEDIUM`は12以上、`LOW`は各rating system内の最大age hint以上をblocked候補へ投影し、`NONE`は空集合にする。`ARIB_EXCEPTIONAL`は単一rating `BROADCASTER_DEFINED`を`contentAgeHint=12`で公開するため、既存policyでは`HIGH/MEDIUM/LOW`の各presetでblocked候補に含まれる。ここでの12はproduct preset policy分類用metadataであり、ARIB raw `parental_rating_descriptor.rating`を12歳へ解釈・変換した値ではない。明示受信した`0x12..0xFF`は従来どおり全て同一canonical exceptional ratingへ写像する。`CUSTOM`はstock TV Appの通常blocked-rating編集を正とし、このextensionがprivate stateを読んで強制上書きしない。第二policy APK、TV App private state reader、System TV App source patchを追加せず、PIN認証済みcurrent contentの`onUnblockContent()`一時解除と他domain/ratingSystemのpolicyを変更しない。TISはraw値から独自policyを実装せず`TvInputManager.isRatingBlocked()`の結果だけに従う。""",
    "F11設計段落",
)

design_path.write_text(design)

integration_path = Path("tis/INTEGRATION.md")
integration = integration_path.read_text()
integration = replace_one(
    integration,
    r"## ARIB exceptional ratingのLive TV App最小policy統合\n.*?(?=\n## MediaSync Exact-mode platform統合)",
    """## ARIB exceptional ratingのLive TV App標準extension統合

JPN parental rating raw `0x12..0xFF` はTISで年齢値へ推測変換せず、`com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED`へ写像する。rating定義とTV Appへの発見経路は独立`AribContentRatings` APKがTIF標準providerとして所有し、blocked-rating policyのownerはLive TV Appとする。System TV App本体へ直接patchを当てる方式は採用しない。

Android 15 / LineageOS 22.1系の既存`ContentRatingLevelPolicy`は、TIF rating-provider XMLの`contentAgeHint`をpreset policyの入力として使う。`HIGH`は6以上、`MEDIUM`は12以上、`LOW`はrating system内の最大age hint以上をblocked候補へ含め、`NONE`は空集合にする。単一ratingである`BROADCASTER_DEFINED`は`contentAgeHint=12`で公開し、stock policyでは`HIGH/MEDIUM/LOW`の各presetでblocked候補になる。12はproduct preset policy分類用metadataであり、ARIB raw `0x12..0xFF`の年齢解釈ではない。`CUSTOM`ではstock TV Appの通常rating設定から同canonical ratingを追加・削除する。第二policy APK、TV App private state reader、`packages/apps/TV` source patchは追加しない。

製品buildでは`AribContentRatings`とstock `LiveTv`を組み込み、rating-provider receiverとXML metadataを発見可能にする。product treeではplatform-signed `AribContentRatingsTvAppIntegrationTests`を`LiveTv`へinstrumentし、canonical ratingについて`TvInputManager.addBlockedRating()` / `removeBlockedRating()` / `isRatingBlocked()`が同一authorityで動作し、試験後にblocked状態を復元できることを確認する。

```text
m AribContentRatings AribContentRatingsTvAppIntegrationTests
atest AribContentRatingsTvAppIntegrationTests
```

TISは引き続き`TvInputManager.isRatingBlocked()`だけをcurrent policy authorityとして扱う。既存のblocked-rating永続化、PIN認証後のsession-level `onUnblockContent()`、通常年齢rating、第三者custom ratingの扱いは変更しない。
""".rstrip(),
    "F11統合節",
)
integration_path.write_text(integration)
