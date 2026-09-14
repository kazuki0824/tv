// SPDX-License-Identifier: GPL-3.0-only
#include "Section.h"

namespace maleicacid::cas {
namespace {

Result sectionBody(const Bytes& section, View* body) {
    if (section.size() < 12 || section.size() > 4096) return Result::BadValue;
    const size_t length = ((section[1] & 0x0f) << 8) | section[2];
    if (length + 3 != section.size() || (section[1] & 0xb0) != 0xb0 ||
        (section[5] & 0xc0) != 0xc0 || section[6] > section[7] ||
        sectionCrc({section.data(), section.size()}) != 0) return Result::BadValue;
    if ((section[5] & 1) == 0) return Result::Unsupported;
    *body = {section.data() + 8, section.size() - 12};
    return Result::Ok;
}

}  // namespace

uint32_t sectionCrc(View input) {
    uint32_t crc = 0xffffffff;
    for (size_t i = 0; i < input.size; ++i) {
        crc ^= static_cast<uint32_t>(input[i]) << 24;
        for (int bit = 0; bit < 8; ++bit) {
            crc = (crc << 1) ^ ((crc & 0x80000000) ? 0x04c11db7 : 0);
        }
    }
    return crc;
}

Result ecmPayload(const Bytes& section, View* payload) {
    if (payload == nullptr) return Result::BadValue;
    *payload = {};
    View body;
    const auto result = sectionBody(section, &body);
    if (result != Result::Ok) return result;
    if (section[0] != 0x82) return Result::Unsupported;
    if (body.size < 30 || body.size > 256) return Result::BadValue;
    *payload = body;
    return Result::Ok;
}

Result emmMessages(const Bytes& section, std::vector<EmmMessage>* messages) {
    if (messages == nullptr) return Result::BadValue;
    messages->clear();
    View body;
    const auto result = sectionBody(section, &body);
    if (result != Result::Ok) return result;
    const bool individual = section[0] == 0x85;
    if (section[0] != 0x84 && !individual) return Result::Unsupported;
    // 共通メッセージは個別メッセージの復号器へ渡さない。
    if (individual && be16(section.data() + 3) != 0) return Result::Unsupported;
    if (body.size == 0) return Result::BadValue;
    std::vector<EmmMessage> parsed;
    size_t offset = 0;
    while (offset < body.size) {
        const size_t header = individual ? 8 : 7;
        if (body.size - offset < header) return Result::BadValue;
        const auto* data = body.data + offset;
        const size_t size = header + (individual ? be16(data + 6) : data[6]);
        if (size < (individual ? 25u : 17u) || size > 256 || size > body.size - offset) {
            return Result::BadValue;
        }
        parsed.push_back({{data, size}, individual});
        offset += size;
    }
    *messages = std::move(parsed);
    return Result::Ok;
}

}  // namespace maleicacid::cas
