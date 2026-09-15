# AOSP 標準 MediaCasService と vendor plugin を製品へ取り込む。
# Yakisoba credential はCAS backendの固定入力であり、実値をこのリポジトリへ置かない。
# 実ファイルのコピー、owner/group/mode、SELinux labelは上位の製品統合設定が担当する。
PRODUCT_PACKAGES += \
    com.android.hardware.cas \
    libmaleicacid_b25_cas
