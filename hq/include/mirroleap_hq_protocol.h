/* MirroLeap experimental decoded video frame IPC, protocol v1.
 * Copyright (c) 2026 MirroLeap contributors. MIT license applies.
 *
 * The producer is an opt-in addition to the Apache-2.0 scrcpy v4.0 client.
 * The wire format is BYTE-OFFSET based, not a compiler-packed struct.
 * Do not send map names or frame contents to a remote server or logs.
 *
 * Single writer, single reader. Slots use a 32-bit odd/even sequence:
 *   InterlockedIncrement => odd (writing)
 *   copy metadata + packed pixels; MemoryBarrier()
 *   InterlockedIncrement => even (committed)
 *   InterlockedExchange(last_slot,slot)
 * The reader checks sequence before/after copying. Never use an odd frame.
 */
#ifndef MIRROLEAP_HQ_PROTOCOL_H
#define MIRROLEAP_HQ_PROTOCOL_H

#include <stdint.h>
#include <stddef.h>

#define MLHQ_VERSION        1u
#define MLHQ_HEADER_BYTES   64u
#define MLHQ_SLOT_BYTES     80u
#define MLHQ_SLOT_COUNT     2u
#define MLHQ_MAX_WIDTH      3840u
#define MLHQ_MAX_HEIGHT     2160u
/* Packed even-dimension 4:2:0: Y plane + (U,V) quarter planes. */
#define MLHQ_SLOT_CAPACITY  (MLHQ_MAX_WIDTH * MLHQ_MAX_HEIGHT * 3u / 2u)
#define MLHQ_MAPPING_BYTES  (MLHQ_HEADER_BYTES + MLHQ_SLOT_COUNT * (MLHQ_SLOT_BYTES + MLHQ_SLOT_CAPACITY))
#define MLHQ_FORMAT_YUV420P 1u
#define MLHQ_FORMAT_NV12    2u
#define MLHQ_NO_FRAME       0xFFFFFFFFu

/* Global header, first 64 bytes. */
#define MLHQ_G_MAGIC        0u  /* bytes 'M','L','H','Q' */
#define MLHQ_G_VERSION      4u
#define MLHQ_G_MAPPING_LEN  8u
#define MLHQ_G_CAPACITY     12u
#define MLHQ_G_SLOT_COUNT   16u
#define MLHQ_G_LATEST_SLOT  20u /* InterlockedExchange */
#define MLHQ_G_LATEST_SEQ   24u /* optional monotonic frame counter */
#define MLHQ_G_RESERVED     28u

/* Each 80-byte slot header is followed by slot_capacity bytes. */
#define MLHQ_S_SEQUENCE     0u  /* odd during write, even when committed */
#define MLHQ_S_FORMAT       4u
#define MLHQ_S_WIDTH        8u
#define MLHQ_S_HEIGHT       12u
#define MLHQ_S_PTS          16u /* signed int64, FFmpeg timebase in caller */
#define MLHQ_S_STRIDE0      24u
#define MLHQ_S_STRIDE1      28u
#define MLHQ_S_STRIDE2      32u
#define MLHQ_S_OFFSET0      36u
#define MLHQ_S_OFFSET1      40u
#define MLHQ_S_OFFSET2      44u
#define MLHQ_S_LENGTH0      48u
#define MLHQ_S_LENGTH1      52u
#define MLHQ_S_LENGTH2      56u
#define MLHQ_S_PAYLOAD      60u
#define MLHQ_S_ROTATION     64u /* 0,90,180,270: currently source-as-decoded */
#define MLHQ_S_RESERVED     68u

#define MLHQ_SLOT_BASE(i)   (MLHQ_HEADER_BYTES + (i) * (MLHQ_SLOT_BYTES + MLHQ_SLOT_CAPACITY))
#define MLHQ_DATA_BASE(i)   (MLHQ_SLOT_BASE(i) + MLHQ_SLOT_BYTES)

/* Additional runtime limit: never build names directly from a device ID. */
#define MLHQ_MAP_PREFIX     L"Local\\MirroLeapHQ_"

#endif /* MIRROLEAP_HQ_PROTOCOL_H */
