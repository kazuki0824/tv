from pathlib import Path

integration = Path("tis/INTEGRATION.md")
text = integration.read_text()

# F11 canonical section: remove the nonexistent integration test module and keep
# the real product/build/runtime checks only.
old = """製品buildでは`AribContentRatings`とstock `LiveTv`を組み込み、rating-provider receiverとXML metadataを発見可能にする。product treeではplatform-signed `AribContentRatingsTvAppIntegrationTests`を`LiveTv`へinstrumentし、canonical ratingについて`TvInputManager.addBlockedRating()` / `removeBlockedRating()` / `isRatingBlocked()`が同一authorityで動作し、試験後にblocked状態を復元できることを確認する。

```text
m AribContentRatings AribContentRatingsTvAppIntegrationTests
atest AribContentRatingsTvAppIntegrationTests
```
"""
new = """製品buildでは`AribContentRatings`とstock `LiveTv`を組み込み、rating-provider receiverとXML metadataを発見可能にする。repo内の静的確認ではprovider package、`ACTION_QUERY_CONTENT_RATING_SYSTEMS`、`META_DATA_CONTENT_RATING_SYSTEMS`、`ARIB_EXCEPTIONAL / BROADCASTER_DEFINED`、`contentAgeHint=12`を確認し、System TV App本体へのARIB専用source patchを要求しない。product tree / 実機ではrating provider discovery、parental controls無効、`NONE`、`HIGH/MEDIUM/LOW`、`CUSTOM`での標準blocked-rating編集、PIN unblock、他domain/ratingSystem非干渉を確認する。

```text
m AribContentRatings LiveTv
```
"""
if old in text:
    text = text.replace(old, new, 1)
elif new not in text:
    raise SystemExit("F11 canonical integration section not found")

old = """System TV App本体は本repoの所有物ではないため、同等実装を本repoへ複製しない。製品platform統合ではSystem TV App側に、上記product固有exceptional ratingだけを対象とするpolicy patchを含めることを必須条件とする。parental controlsが有効でglobal policyが`NONE`以外の場合はこのratingをglobal blocked-rating集合へ反映する一方、PIN認証済みの現在コンテンツに対する `onUnblockContent()` 一時解除は維持する。第三者custom rating、CTS Verifierが提供するrating、他domain/ratingSystemのblock/unblock可否へこのpolicyを波及させない。"""
new = """System TV App本体へARIB専用source patchを追加しない。`AribContentRatings`のTIF標準rating-provider metadataをstock TV Appが読み込み、`contentAgeHint=12`を既存preset policyへ適用する。`HIGH/MEDIUM/LOW`ではexceptional ratingがblocked候補に含まれ、`NONE`では含まれない。`CUSTOM`はstock TV Appの通常blocked-rating編集を使う。PIN認証済みの現在コンテンツに対する`onUnblockContent()`一時解除を維持し、第三者custom rating、CTS Verifier由来rating、他domain/ratingSystemのblock/unblock可否へこのproduct metadataを波及させない。"""
if old in text:
    text = text.replace(old, new, 1)
elif new not in text:
    raise SystemExit("F11 legacy policy-patch requirement not found")

old = "- System TV AppのARIB exceptional policyが有効で、PINによる現在コンテンツ一時解除と第三者rating非干渉を維持する。"
new = "- `AribContentRatings`がstock TV Appから発見され、既存preset/custom policyを通じてARIB exceptional ratingのblock/unblockとPIN一時解除、第三者rating非干渉が成立する。"
if old in text:
    text = text.replace(old, new, 1)
elif new not in text:
    raise SystemExit("F11 legacy acceptance gate not found")

# Ensure F15's canonical design text is already present; do not rewrite it here.
design = Path("tis/DESIGN_JA.md").read_text()
needle = "frontend/HALがdynamic stream-ID discoveryという能力自体を提供しないことをframeworkの`Tuner.RESULT_UNAVAILABLE`として明示した場合に限り"
if needle not in design:
    raise SystemExit("F15 RESULT_UNAVAILABLE-only fallback contract missing")

# Static F11 facts must match the canonical documentation.
from xml.etree import ElementTree
ns = "{http://schemas.android.com/apk/res/android}"
root = ElementTree.parse("tis/arib_parental_rating/res/xml/tv_content_rating_systems.xml").getroot()
system = root.find("rating-system-definition")
if system is None or system.get(ns + "name") != "ARIB_EXCEPTIONAL":
    raise SystemExit("ARIB_EXCEPTIONAL rating system missing")
rating = system.find("rating-definition")
if rating is None or rating.get(ns + "name") != "BROADCASTER_DEFINED":
    raise SystemExit("BROADCASTER_DEFINED rating missing")
if rating.get(ns + "contentAgeHint") != "12":
    raise SystemExit("contentAgeHint must remain 12")
if Path("tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch").exists():
    raise SystemExit("legacy direct System TV App patch still exists")

integration.write_text(text)
