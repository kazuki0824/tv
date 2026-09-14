// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include "Types.h"

namespace maleicacid::cas {

struct EmmMessage {
    View payload;
    bool individual;
};

uint32_t sectionCrc(View input);
Result ecmPayload(const Bytes& section, View* payload);
Result emmMessages(const Bytes& section, std::vector<EmmMessage>* messages);

}  // namespace maleicacid::cas
