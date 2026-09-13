# vendor/maleicacid/tv のBoardConfig統合入口。
# device側は個別componentのsepolicy directoryを列挙しない。

include vendor/maleicacid/tv/tuner_hal2/config/BoardConfigVendorSePolicy.mk

# production CAS sepolicy は PR #57 が所有する。mainではfileが存在しないため無効。
-include vendor/maleicacid/tv/cas_hal/config/BoardConfigVendorSePolicy.mk
