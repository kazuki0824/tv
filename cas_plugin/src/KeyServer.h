// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <atomic>
#include <memory>
#include <thread>

namespace maleicacid::cas {

// CAS process 内の読取り口。session の所有者にはならず、更新要求も受け付けない。
class KeyServer {
public:
    static std::shared_ptr<KeyServer> acquire();
    ~KeyServer();
    bool healthy() const { return healthy_.load(); }

private:
    bool start();
    void run();
    void serve(int fd);
    int fd_ = -1;
    int ownerFd_ = -1;
    std::thread thread_;
    std::atomic<bool> stop_{false};
    std::atomic<bool> healthy_{false};
};

}  // namespace maleicacid::cas
