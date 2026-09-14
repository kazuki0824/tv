// SPDX-License-Identifier: GPL-3.0-only
#include "KeySocket.h"

#include <cstdio>
#include <cstring>
#include <poll.h>
#include <unistd.h>

namespace maleicacid::cas {

socklen_t keySocketAddress(sockaddr_un* address) {
    *address = {};
    address->sun_family = AF_UNIX;
#ifdef MALEICACID_CAS_TEST
    // host/atest 専用 target の名前空間は製品 listener と共有しない。
    const auto count = snprintf(address->sun_path + 1, sizeof(address->sun_path) - 1,
                                "maleicacid.cas.test.%d", getpgrp());
#else
    const auto count = snprintf(address->sun_path + 1, sizeof(address->sun_path) - 1,
                                "maleicacid.cas.b25");
#endif
    return static_cast<socklen_t>(offsetof(sockaddr_un, sun_path) + 1 + count);
}

bool peerMatchesPolicy(uid_t uid, const char* label, bool server) {
    if (uid != (server ? 1013u : 1000u) || label == nullptr) return false;
    return strcmp(label, server ? "u:r:hal_cas_default:s0" : "u:r:hal_tv_tuner_default:s0") == 0;
}

bool authorizedPeer(int fd, bool server) {
    struct ucred credential {};
    socklen_t size = sizeof(credential);
    if (getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &credential, &size) || size != sizeof(credential)) {
        return false;
    }
#ifdef MALEICACID_CAS_TEST
    (void)server;
    return credential.uid == geteuid();
#else
    // 同じ media UID を使う別 HAL を、鍵の読取り主体として認可しない。
    char label[256]{};
    size = sizeof(label);
    if (getsockopt(fd, SOL_SOCKET, SO_PEERSEC, label, &size) || size == 0 || size >= sizeof(label)) {
        return false;
    }
    label[size] = '\0';
    return peerMatchesPolicy(credential.uid, label, server);
#endif
}

bool waitSocket(int fd, short events) {
    pollfd item{fd, events, 0};
    // EINTR でも無制限再試行せず、当該参照取得を失敗させる。
    return poll(&item, 1, kSocketDeadlineMs) == 1 && (item.revents & events) != 0;
}

}  // namespace maleicacid::cas
