# AOSP 標準 MediaCasService と vendor plugin を製品へ取り込む。
PRODUCT_PACKAGES += \
    com.android.hardware.cas \
    libmaleicacid_b25_cas \
    fs_config_files

# 製品側はこの makefile の継承前に、管理下の credential 入力を指定する。
# 未指定時はファイルを生成しない。鍵がない状態の扱いは backend に委ねる。
ifneq ($(strip $(MALEICACID_BCAS_KEYS_FILE)),)
ifneq ($(words $(MALEICACID_BCAS_KEYS_FILE)),1)
$(error MALEICACID_BCAS_KEYS_FILE must name one file without whitespace)
endif
ifneq ($(wildcard $(strip $(MALEICACID_BCAS_KEYS_FILE))),$(strip $(MALEICACID_BCAS_KEYS_FILE)))
$(error MALEICACID_BCAS_KEYS_FILE must name an existing file without wildcard characters)
endif
PRODUCT_COPY_FILES += \
    $(strip $(MALEICACID_BCAS_KEYS_FILE)):$(TARGET_COPY_OUT_VENDOR)/etc/maleicacid/bcas_keys
endif
