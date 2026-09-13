# virtio_x86_64_tv_grub系productから利用するTV stack統合。
# component package一覧のSSOTは各componentのproduct_integration.mkに置く。

$(call inherit-product, vendor/maleicacid/tv/config/product_integration.mk)

# PX4を使うこのproduct familyに必要な実機データの配置も、device product側へ
# 個別のPRODUCT_COPY_FILESを書かずここで一元化する。
PRODUCT_COPY_FILES += \
    device/maleicacid/virtio_x86_64_tv_grub/px4_drv/etc/it930x-firmware.bin:$(TARGET_COPY_OUT_VENDOR)/firmware/it930x-firmware.bin \
    vendor/maleicacid/tv/config/px4/init.px4_drv.rc:$(TARGET_COPY_OUT_VENDOR)/etc/init/init.px4_drv.rc \
    vendor/maleicacid/tv/config/ueventd.virtio_x86_64_tv_grub.rc:$(TARGET_COPY_OUT_VENDOR)/ueventd.rc
