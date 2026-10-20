/* Optional MirroLeap HQ decoded video frame sink for scrcpy v4.0.
 * Copyright (c) 2026 MirroLeap contributors. This file: MIT license.
 * Patches to original Genymobile/scrcpy retain upstream Apache-2.0 notices.
 *
 * Only enabled when a parent process owns the named memory mapping and
 * supplies MIRROLEAP_HQ_TOKEN. All failures degrade to normal scrcpy video.
 */
#ifndef SC_MIRROLEAP_HQ_SINK_H
#define SC_MIRROLEAP_HQ_SINK_H
#include "common.h"
#include "trait/frame_sink.h"

#ifdef _WIN32
#include <windows.h>
#include <stdint.h>

struct sc_mirroleap_hq_sink {
    struct sc_frame_sink frame_sink; /* first member: trait upcast */
    HANDLE mapping;
    unsigned char *memory;
    HANDLE worker;
    HANDLE ready;
    volatile LONG stopping;
    void * volatile newest; /* owned AVFrame* via InterlockedExchangePointer */
    uint32_t next_slot;
};
void sc_mirroleap_hq_sink_init(struct sc_mirroleap_hq_sink *sink);
#endif

#endif
