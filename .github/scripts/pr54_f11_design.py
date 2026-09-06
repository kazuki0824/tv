from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one target, found {count}")
    p.write_text(text.replace(old, new, 1))


replace_once(
    "tis/DESIGN_JA.md",
    "- System TV Appはpolicy ownerとして、parental controlsが有効でglobal policyが`NONE`以外の場合に限り上記exceptional ratingをblocked-rating集合へ反映する。PIN認証済みcurrent contentの `onUnblockContent()` 一時解除は維持し、第三者custom rating、CTS Verifier由来rating、他domain/ratingSystemへこのproduct policyを波及させない。TISはraw値から独自policyを実装せず `TvInputManager.isRatingBlocked()` の結果だけに従う。",
    "- ARIB exceptional ratingのpolicy ownerはLive TV Appとする。rating定義は独立`AribContentRatings` APKがTIF標準providerとして公開する。Android 15 / LineageOS 22.1 baselineではpartner customizationとRRO/product overlayはresource customizationに限定され、`ContentRatingLevelPolicy`またはblocked-rating projectionへ外部codeを注入する正式hookがないため、product integrationではLive TV Appの既存parental policy pathへ `ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` 1件のprojectionだけを最小差分で統合する。第二policy APKを追加せず、TISや別APKからLive TV App privateの`NONE/HIGH/MEDIUM/LOW/CUSTOM` stateを読む設計にしない。parental controls無効またはglobal policy=`NONE`ではexceptional ratingをblocked集合へ入れず、parental controls有効かつ`HIGH/MEDIUM/LOW/CUSTOM`ではblocked集合へ反映する。PIN認証済みcurrent contentの `onUnblockContent()` 一時解除は維持し、第三者custom rating、CTS Verifier由来rating、他domain/ratingSystemへこのproduct policyを波及させない。TISはraw値から独自policyを実装せず `TvInputManager.isRatingBlocked()` の結果だけに従う。",
)

integration = Path("tis/INTEGRATION.md")
text = integration.read_text()
start = text.index("## ARIB exceptional parental rating extension統合\n")
end = text.index("## MediaSync Exact-mode platform統合\n", start)
section = """## ARIB exceptional ratingのLive TV App最小policy統合

JPN parental rating raw `0x12..0xFF` はTISで年齢値へ推測変換せず、`com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` へ写像する。rating定義とTV Appへの発見経路は独立`AribContentRatings` APKがTIF標準providerとして所有し、blocked-rating policyのownerはLive TV Appとする。

Android 15 / LineageOS 22.1 baselineのpartner customization、RRO、product resource overlayはresource customizationに限定され、`ContentRatingLevelPolicy`またはblocked-rating projectionへ外部codeを注入する正式hookを持たない。そのため第二policy APKは追加せず、Live TV Appの既存parental policy pathへexceptional rating 1件のprojectionだけを最小product integrationとして加える。TISまたは別APKがLive TV App privateの`NONE/HIGH/MEDIUM/LOW/CUSTOM` stateを読む構成にはしない。実装差分は `tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch` を正規product pathとする。

policy mappingは、parental controls無効またはglobal policy=`NONE`ならexceptional ratingをblocked集合へ入れず、parental controls有効かつ`HIGH/MEDIUM/LOW/CUSTOM`ならblocked集合へ反映する。既存のblocked-rating永続化、PIN認証後のsession-level `onUnblockContent()`、通常年齢rating、第三者custom ratingの扱いは変更しない。TISは `TvInputManager.isRatingBlocked()` を唯一のpolicy authorityとして使い続ける。

製品統合ではpatch適用後のLive TV App target compileに加え、`AribContentRatings` discovery、parental controls無効、`NONE`、`HIGH/MEDIUM/LOW/CUSTOM`、PIN unblock、他domain/ratingSystem非干渉を確認する。

"""
integration.write_text(text[:start] + section + text[end:])
