# AOSP 標準 MediaCasService と vendor plugin を製品へ取り込む。
# credential は製品管理下で /vendor/etc/maleicacid/bcas_keys へ配置する。
# root 所有、0640 以下、media group に読取りを許可し、他者の書込みは許可しない。
PRODUCT_PACKAGES += \
    com.android.hardware.cas \
    libmaleicacid_b25_cas
