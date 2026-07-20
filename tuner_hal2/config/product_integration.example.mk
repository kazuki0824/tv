# 製品のproduct makefileから次を継承する。
# $(call inherit-product, vendor/maleicacid/tv/tuner_hal2/config/product_integration.mk)
PRODUCT_PACKAGES += \
    android.hardware.tv.tuner-service.maleicacid2 \
    maleicacid_tuner_hal2_ueventd_rc
