// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include "KeyRegistry.h"
#include "Section.h"
#include "YakisobaState.h"

#include <mutex>
#include <string>

namespace maleicacid::cas {

class YakisobaBackend {
public:
    static YakisobaBackend& instance();
    Result processEcm(const std::shared_ptr<KeyRegistry::Slot>& slot, View payload);
    Result processEmm(const std::vector<EmmMessage>& messages);
    Result resolve(const Token& token, Secret<16>* keys);
#ifdef MALEICACID_CAS_TEST
    void setCredentialPathForTest(std::string path);
#endif
private:
    Result initialize();
    Result restorePersistentState(const std::array<uint8_t, 6>& cardId);
    Result applyMessage(const EmmMessage& message);
    Result validateEcm(View plain, const PersistedEntitlement& entitlement);
    Result failClosed();

    std::timed_mutex mutex_;
    bool initialized_ = false;
    bool revoked_ = false;
    std::string credentialPath_ = "/vendor/etc/maleicacid/bcas_keys";
    YakisobaStateStore stateStore_;
    YakisobaPersistentState state_;
};

}  // namespace maleicacid::cas
