#pragma once

#include <aidl/android/hardware/cas/IMediaCasService.h>
#include <memory>

namespace maleicacid::cas {
using Service = aidl::android::hardware::cas::IMediaCasService;

// AOSP ClearKeyとRustのproduct capabilityを起動時に合成する。
// ClearKeyが欠落/重複する場合はserviceを公開しない。
std::shared_ptr<Service> make_service_boundary(
        std::shared_ptr<Service> product, std::shared_ptr<Service> reference);
}  // namespace maleicacid::cas
