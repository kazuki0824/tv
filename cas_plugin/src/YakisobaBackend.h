// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include "KeyRegistry.h"
#include "Section.h"

#include <mutex>
#include <string>

namespace maleicacid::cas {

// libyakisoba の共有台帳へ到達する唯一の所有者。ロック順は backend → registry。
class YakisobaBackend {
public:
    static YakisobaBackend& instance();
    Result processEcm(const std::shared_ptr<KeyRegistry::Slot>& slot, View payload);
    Result processEmm(const std::vector<EmmMessage>& messages);
#ifdef MALEICACID_CAS_TEST
    Result resolve(const Token& token, Secret<16>* keys);
#endif
    Result bind(const Token& token, int* readerFd);
#ifdef MALEICACID_CAS_TEST
    void setCredentialPathForTest(std::string path);
#endif

private:
    struct Entitlement {
        bool updated = false;
        uint16_t number = 0;
        uint32_t expires = 0xffff;
        std::array<uint8_t, 32> bitmap{};
        Bytes lastMessage;
    };

    Result initialize();
    Result applyMessage(const EmmMessage& message);
    Result validateEcm(View plain, const Entitlement& entitlement);
    Result failClosed();

    std::timed_mutex mutex_;
    bool initialized_ = false;
    bool revoked_ = false;
    std::string credentialPath_ = "/vendor/etc/maleicacid/bcas_keys";
    std::array<Entitlement, 6> entitlements_;
};

}  // namespace maleicacid::cas
