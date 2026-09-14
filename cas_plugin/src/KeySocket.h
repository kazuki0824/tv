// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <cstddef>
#include <sys/socket.h>
#include <sys/un.h>

namespace maleicacid::cas {

constexpr int kSocketDeadlineMs = 100;
socklen_t keySocketAddress(sockaddr_un* address);
bool authorizedPeer(int fd, bool server);
bool peerMatchesPolicy(uid_t uid, const char* label, bool server);
bool waitSocket(int fd, short events);

}  // namespace maleicacid::cas
