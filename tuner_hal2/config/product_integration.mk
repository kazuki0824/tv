# r50ee5以降の既定Tuner HAL統合。
# product defaultはtuner_hal2のみとし、旧tuner_hal service packageは追加しない。
PRODUCT_PACKAGES += \
    android.hardware.tv.tuner-service.maleicacid2 \
    maleicacid_tuner_hal2_ueventd_rc
