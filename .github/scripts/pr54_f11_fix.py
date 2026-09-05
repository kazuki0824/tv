from pathlib import Path
import shutil

root = Path("tis")
old = root / "arib_content_ratings"
new = root / "arib_parental_rating"
if new.exists():
    raise SystemExit("destination already exists")
shutil.copytree(old, new)

# The extension is a standard TIF rating provider discovered by TV App; keep its module
# name stable so product_integration.mk does not gain a second policy owner.
bp = new / "Android.bp"
text = bp.read_text()
text = text.replace(
    'android_app {\n    name: "AribContentRatings",',
    '// AOSP TIF標準のrating-system providerとしてTV Appから発見されるproduct extension。\nandroid_app {\n    name: "AribContentRatings",',
)
bp.write_text(text)

# Remove the direct System TV App source patch: the standard rating provider is the
# customization boundary and TvInputManager blocked-ratings remains policy authority.
patch = root / "platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch"
patch.unlink()
shutil.rmtree(old)

integration = root / "INTEGRATION.md"
text = integration.read_text()
old_section = '''## System TV App exceptional rating policy統合

JPN parental rating raw `0x12..0xFF` はTISで `com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` として保持する。blocked-rating policyのownerはSystem TV Appなので、LineageOS 22.1 / Android 15 product treeの `packages/apps/TV` へ `tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch` を適用する。

```bash
cd packages/apps/TV
git apply --check "$TV_REPO/tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch"
git apply "$TV_REPO/tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch"
```

patchはparental controls有効かつglobal rating levelが`NONE`以外の場合だけ当該product固有ratingをblocked集合へ追加し、disabled/`NONE`では当該ratingだけを除去する。TISは`TvInputManager.isRatingBlocked()`をpolicy authorityとして使い続ける。製品統合ではpatch適用後のSystem TV App target compileとenable/disable、`NONE`/非`NONE`、PIN unblockを確認する。
'''
new_section = '''## ARIB exceptional parental rating extension統合

JPN parental rating raw `0x12..0xFF` はTISで年齢値へ推測変換せず、`com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` へ写像する。rating systemの公開とTV App parental-control UIへの発見経路は、`tis/arib_parental_rating/` の独立product extension `AribContentRatings` を正規経路とする。System TV App本体のsourceはpatchしない。

`AribContentRatings` はAOSP TIF標準の `android.media.tv.action.QUERY_CONTENT_RATING_SYSTEMS` receiverと `android.media.tv.metadata.CONTENT_RATING_SYSTEMS` XMLだけを公開する。`BROADCASTER_DEFINED` は `contentAgeHint=0` とし、TV App側のrating-level policyが標準rating metadataからblocked-rating集合を構成できるようにする。TIS自身はblocked集合を変更せず、再生時は従来どおり `TvInputManager.isRatingBlocked()` を唯一のpolicy authorityとして使用する。

product統合では `tis/config/product_integration.mk` を継承して `/product/app/AribContentRatings/AribContentRatings.apk` を組み込み、次を確認する。

```text
- PackageManager/TvInputManager経由で com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED が列挙される。
- parental-control UIにARIB exceptional rating systemが表示される。
- parental controls無効時はTISが当該ratingを独自blockしない。
- TV App/TvInputManagerのblocked-rating集合に当該ratingがある場合だけ、TISの isRatingBlocked() 判定から notifyContentBlocked() へ到達する。
- PINによる一時unblock後にTIS独自policyが再blockしない。
- raw 0x12..0xFF を通常年齢ratingまたはUNRATEDへ変換しない。
```

このextensionはrating定義・発見だけを所有し、blocked-rating集合の所有権をTISへ移さない。そのためSystem TV Appのprivate stateやprivate APIへの依存、packages/apps/TV source patch、TIS内の第二parental policyを追加しない。
'''
if text.count(old_section) != 1:
    raise SystemExit(f"INTEGRATION section match count={text.count(old_section)}")
text = text.replace(old_section, new_section)
text = text.replace("`tis/arib_content_ratings/Android.bp`", "`tis/arib_parental_rating/Android.bp`")
integration.write_text(text)
