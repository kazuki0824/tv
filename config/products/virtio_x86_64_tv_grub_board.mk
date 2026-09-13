# virtio_x86_64_tv_grubのTV stack / PX4 BoardConfig統合。

include vendor/maleicacid/tv/config/BoardConfigVendorSePolicy.mk

# px4_drvはproductと同じkernel source/config/output/toolchainに対するexternal kbuildで生成する。
TARGET_KERNEL_EXT_MODULE_ROOT := kernel/maleicacid
TARGET_KERNEL_EXT_MODULES += \
    px4_drv:kbuild
