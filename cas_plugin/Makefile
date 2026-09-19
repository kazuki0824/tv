# host 検証用。製品は Android.bp の libyakisoba 静的依存を使う。
# make -C cas_plugin test YAKISOBA_DIR=/path/to/libyakisoba-cross AOSP_HEADERS=/path/to/headers
CXX ?= g++
CC ?= gcc
YAKISOBA_DIR ?= ../../libyakisoba
AOSP_HEADERS ?= /tmp/maleicacid-cas-aosp-headers
BUILD_DIR ?= /tmp/maleicacid-cas-build
CPPFLAGS := -Iinclude -Isrc -Itests/host_include -I$(AOSP_HEADERS) -I$(YAKISOBA_DIR)/src
CXXFLAGS := -std=c++17 -Wall -Wextra -Werror -fPIC -pthread -O2 -g -fvisibility=hidden
CAS_SOURCES := src/CasPlugin.cpp src/Section.cpp src/YakisobaBackend.cpp src/YakisobaState.cpp src/KeyRegistry.cpp src/KeyServer.cpp src/KeySocket.cpp
YAKISOBA_SOURCES := Crypto Decoder-ECM Decoder-EMM Keyset
YAKISOBA_OBJECTS := $(addprefix $(BUILD_DIR)/yakisoba/,$(addsuffix .o,$(YAKISOBA_SOURCES)))

.PHONY: all test test-core clean
all: $(BUILD_DIR)/libmaleicacid_b25_cas.so $(BUILD_DIR)/maleicacid_b25_cas_test $(BUILD_DIR)/maleicacid_b25_state_test

$(BUILD_DIR)/yakisoba/%.o: $(YAKISOBA_DIR)/src/%.c
	mkdir -p $(@D)
	$(CC) -std=c99 -Wall -fvisibility=hidden -fPIC -O2 -g -c $< -o $@

$(BUILD_DIR)/libmaleicacid_b25_cas.so: $(CAS_SOURCES) $(wildcard src/*.h) $(YAKISOBA_OBJECTS) exports.map
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) $(CAS_SOURCES) $(YAKISOBA_OBJECTS) -shared -Wl,--wrap=fopen,--version-script=exports.map,-z,defs -o $@

$(BUILD_DIR)/maleicacid_b25_cas_test: $(CAS_SOURCES) src/KeyClient.cpp tests/CasTest.cpp $(wildcard src/*.h include/maleicacid/cas/*.h) $(YAKISOBA_OBJECTS)
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -DMALEICACID_CAS_TEST $(CAS_SOURCES) src/KeyClient.cpp tests/CasTest.cpp $(YAKISOBA_OBJECTS) -Wl,--wrap=fopen -o $@

$(BUILD_DIR)/maleicacid_b25_state_test: src/YakisobaState.cpp src/YakisobaState.h src/Types.h tests/YakisobaStateTest.cpp
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -DMALEICACID_CAS_TEST src/YakisobaState.cpp tests/YakisobaStateTest.cpp -o $@

test: all
	$(BUILD_DIR)/maleicacid_b25_state_test
	$(BUILD_DIR)/maleicacid_b25_cas_test

# 制限された host でも、socket を代用品へ置き換えず backend 本体を個別に検証する。
test-core: all
	$(BUILD_DIR)/maleicacid_b25_state_test
	$(BUILD_DIR)/maleicacid_b25_cas_test --core

clean:
	rm -rf $(BUILD_DIR)
