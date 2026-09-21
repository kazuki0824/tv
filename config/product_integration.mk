# vendor/maleicacid/tv の製品統合入口。
# product側は個別module名を列挙せず、このfileを継承する。
# 旧 tuner_hal は参照実装のため、product defaultへは含めない。
#
# B25 MULTI2固定parameterはrepository内の40 byte binaryを既定入力とする。
# Yakisoba credentialは実放送を復号できないdummyを既定入力とする。
# 実credentialを使用する製品は、このfileの継承前に
# MALEICACID_BCAS_KEYS_FILEをrepository外または秘密管理されたfileへ上書きする。
MALEICACID_B25_MULTI2_PARAMETERS_FILE ?= vendor/maleicacid/tv/product_inputs/b25_multi2_parameters
MALEICACID_BCAS_KEYS_FILE ?= vendor/maleicacid/tv/product_inputs/bcas_keys

$(call inherit-product, vendor/maleicacid/tv/tuner_hal2/config/product_integration.mk)
$(call inherit-product, vendor/maleicacid/tv/tis/config/product_integration.mk)
$(call inherit-product, vendor/maleicacid/tv/cas_plugin/config/product_integration.mk)
