# 既定Tuner HAL統合。
# product defaultはtuner_hal2のみとし、旧tuner_hal service packageは追加しない。
PRODUCT_PACKAGES += \
    android.hardware.tv.tuner-service.maleicacid2 \
    maleicacid_tuner_hal2_ueventd_rc \
    fs_config_files

# PackageManagerへTuner hardware featureを宣言し、TunerResourceManagerServiceと
# framework Tuner APIの利用条件を成立させる。
PRODUCT_COPY_FILES += \
    frameworks/native/data/etc/android.hardware.tv.tuner.xml:$(TARGET_COPY_OUT_VENDOR)/etc/permissions/android.hardware.tv.tuner.xml

# B25のMULTI2固定parameterは、このmakefileを継承する前に指定する。
# 未指定時は配置しない。標準値やテスト用の代替入力を生成しない。
# B1は別入力として扱い、B25値を流用しない。
ifneq ($(strip $(MALEICACID_B25_MULTI2_PARAMETERS_FILE)),)
ifneq ($(words $(MALEICACID_B25_MULTI2_PARAMETERS_FILE)),1)
$(error MALEICACID_B25_MULTI2_PARAMETERS_FILE must name one file without whitespace)
endif
ifneq ($(wildcard $(strip $(MALEICACID_B25_MULTI2_PARAMETERS_FILE))),$(strip $(MALEICACID_B25_MULTI2_PARAMETERS_FILE)))
$(error MALEICACID_B25_MULTI2_PARAMETERS_FILE must name an existing file without wildcard characters)
endif
PRODUCT_COPY_FILES += \
    $(strip $(MALEICACID_B25_MULTI2_PARAMETERS_FILE)):$(TARGET_COPY_OUT_VENDOR)/etc/maleicacid/b25_multi2_parameters
endif

# VtsEnvironmentProfile compile が生成したvalidated prebuiltだけを取り込む。
# 未解決profileしかない通常buildではファイル自体が存在せず、VTS設定を推測してinstallしない。
_tuner_hal2_config_dir := $(dir $(lastword $(MAKEFILE_LIST)))
-include $(_tuner_hal2_config_dir)generated/vts_product_generated.mk
