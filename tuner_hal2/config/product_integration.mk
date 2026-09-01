# 既定Tuner HAL統合。
# product defaultはtuner_hal2のみとし、旧tuner_hal service packageは追加しない。
PRODUCT_PACKAGES += \
    android.hardware.tv.tuner-service.maleicacid2 \
    maleicacid_tuner_hal2_ueventd_rc

# VtsEnvironmentProfile compile が生成したvalidated prebuiltだけを取り込む。
# 未解決profileしかない通常buildではファイル自体が存在せず、VTS設定を推測してinstallしない。
_tuner_hal2_config_dir := $(dir $(lastword $(MAKEFILE_LIST)))
-include $(_tuner_hal2_config_dir)generated/vts_product_generated.mk
