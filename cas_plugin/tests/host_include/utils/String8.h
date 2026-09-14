// SPDX-License-Identifier: GPL-3.0-only
#pragma once

// host target だけの libutils 代替。CasAPI / MediaErrors 自体は AOSP 原本を使用する。
#include <utils/Errors.h>
#include <cstddef>
#include <cstdint>
#include <string>

namespace android {
class String8 {
public:
    String8() = default;
    explicit String8(const char* value) : value_(value) {}
    const char* c_str() const { return value_.c_str(); }
private:
    std::string value_;
};
}  // namespace android
