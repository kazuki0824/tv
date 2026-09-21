# AOSP標準のTV視聴アプリを製品へ組み込み、TvProviderのchannel URIをOS上で開けるようにする。
PRODUCT_PACKAGES += \
    LiveTv \
    MaleicacidTvInput \
    AribContentRatings \
    privapp-permissions-maleicacid-tvinput \
    libmaleicacid_arib_si_engine_jni \
    libmaleicacid_arib_caption_jni

PRODUCT_COPY_FILES += \
    frameworks/native/data/etc/android.software.live_tv.xml:$(TARGET_COPY_OUT_PRODUCT)/etc/permissions/android.software.live_tv.xml
