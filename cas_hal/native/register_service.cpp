#include "cas_service_boundary.h"

#include <android/binder_manager.h>
#include "MediaCasService.h"

namespace {
std::shared_ptr<maleicacid::cas::Service> registered_service;
}

// rawは呼出中だけ有効な借用。NDK SpAIBinderへ独立したstrong referenceを渡す。
// Rustの内側serviceはservice managerへ登録せず、外側/defaultだけを公開する。
extern "C" int32_t maleicacid_register_cas_service(AIBinder* raw) {
    if (raw == nullptr || registered_service) return STATUS_BAD_VALUE;
    AIBinder_incStrong(raw);
    auto product = maleicacid::cas::Service::fromBinder(ndk::SpAIBinder(raw));
    auto reference = ndk::SharedRefBase::make<aidl::android::hardware::cas::MediaCasService>();
    auto service = maleicacid::cas::make_service_boundary(std::move(product), std::move(reference));
    if (!service) return STATUS_INVALID_OPERATION;
    const auto status = AServiceManager_addService(service->asBinder().get(),
                                                   "android.hardware.cas.IMediaCasService/default");
    if (status == STATUS_OK) registered_service = std::move(service);
    return status;
}
