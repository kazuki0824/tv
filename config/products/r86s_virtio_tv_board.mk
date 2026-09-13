# r86s_virtio_tvのTV stack / PX4 BoardConfig統合。
# 既存device productのprebuilt vendor module archive方式を維持する。

include vendor/maleicacid/tv/config/BoardConfigVendorSePolicy.mk

PX4_DRV_VENDOR_MODULES_ARCHIVE := \
    device/maleicacid/virtio_x86_64_tv_grub/px4_drv/prebuilt/px4_drv_vendor_modules.zip

BOARD_VENDOR_KERNEL_MODULES_ARCHIVE := $(PX4_DRV_VENDOR_MODULES_ARCHIVE)
