/* Optional decoded-frame shared-memory sink for scrcpy v4.0, Windows.
 * Copyright (c) 2026 MirroLeap contributors. MIT licensed.
 * This is an add-on to the Apache-2.0 scrcpy app, not a replacement server.
 */
#include "mirroleap_hq_sink.h"

#ifdef _WIN32

#include <libavutil/frame.h>
#include <libavutil/pixfmt.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#include "mirroleap_hq_protocol.h"
#include "util/log.h"

static void
mlhq_u32(unsigned char *base, size_t offset, uint32_t value) {
    /* Windows / supported x86_64 client is little-endian, as is protocol v1 */
    memcpy(base + offset, &value, sizeof(value));
}

static uint32_t
mlhq_read_u32(const unsigned char *base, size_t offset) {
    uint32_t out;
    memcpy(&out, base + offset, sizeof(out));
    return out;
}

static bool
mlhq_mapping_name(wchar_t *out, size_t capacity) {
    char token[42];
    DWORD length=GetEnvironmentVariableA("MIRROLEAP_HQ_TOKEN",token,
                                          (DWORD)sizeof(token));
    /* Token must be 32 random hex digits; device IDs never enter filenames. */
    if (length!=32 || length>=sizeof(token)) {
        return false;
    }
    const wchar_t prefix[]=MLHQ_MAP_PREFIX;
    const size_t prefix_length=wcslen(prefix);
    if (capacity<=prefix_length+32) {return false;}
    wmemcpy(out,prefix,prefix_length);
    for (size_t i=0;i<32;++i) {
        unsigned char c=(unsigned char)token[i];
        if (!((c>='0' && c<='9') || (c>='a' && c<='f'))) {
            return false;
        }
        out[prefix_length+i]=(wchar_t)c;
    }
    out[prefix_length+32]=L'\0';
    return true;
}

static bool
mlhq_compatible_mapping(const unsigned char *base) {
    return !memcmp(base,"MLHQ",4) &&
           mlhq_read_u32(base,MLHQ_G_VERSION)==MLHQ_VERSION &&
           mlhq_read_u32(base,MLHQ_G_MAPPING_LEN)==MLHQ_MAPPING_BYTES &&
           mlhq_read_u32(base,MLHQ_G_CAPACITY)==MLHQ_SLOT_CAPACITY &&
           mlhq_read_u32(base,MLHQ_G_SLOT_COUNT)==MLHQ_SLOT_COUNT;
}

static bool
mlhq_frame_supported(const AVFrame *frame) {
    if (!frame || frame->width<2 || frame->height<2 ||
        frame->width>(int)MLHQ_MAX_WIDTH ||
        frame->height>(int)MLHQ_MAX_HEIGHT ||
        (frame->width&1) || (frame->height&1)) {
        return false;
    }
    const int w=frame->width;
    if (frame->format==AV_PIX_FMT_YUV420P) {
        return frame->data[0] && frame->data[1] && frame->data[2] &&
               frame->linesize[0]>=w &&
               frame->linesize[1]>=w/2 && frame->linesize[2]>=w/2;
    }
    if (frame->format==AV_PIX_FMT_NV12) {
        return frame->data[0] && frame->data[1] &&
               frame->linesize[0]>=w && frame->linesize[1]>=w;
    }
    return false;
}

static void
mlhq_copy_rows(unsigned char *dest, const uint8_t *src,
               int source_stride, int row_width, int row_count) {
    for (int row=0;row<row_count;++row) {
        memcpy(dest+(size_t)row*(size_t)row_width,
               src+(size_t)row*(size_t)source_stride,(size_t)row_width);
    }
}

/* Called on a dedicated lightweight worker, never on FFmpeg decoder thread. */
static void
mlhq_publish(struct sc_mirroleap_hq_sink *sink,const AVFrame *frame) {
    if (!sink->memory || !mlhq_frame_supported(frame)) {return;}
    const uint32_t w=(uint32_t)frame->width;
    const uint32_t h=(uint32_t)frame->height;
    const uint32_t y=w*h;
    const uint32_t total=y+y/2;
    if (total>MLHQ_SLOT_CAPACITY) {return;}
    const uint32_t index=sink->next_slot++%MLHQ_SLOT_COUNT;
    unsigned char *slot=sink->memory+MLHQ_SLOT_BASE(index);
    unsigned char *data=sink->memory+MLHQ_DATA_BASE(index);
    volatile LONG *sequence=(volatile LONG*)(void*)(slot+MLHQ_S_SEQUENCE);
    InterlockedIncrement(sequence); // even -> odd: frame incomplete

    uint32_t format;
    uint32_t stride1;
    uint32_t stride2;
    uint32_t offset2;
    uint32_t length1;
    uint32_t length2;
    if (frame->format==AV_PIX_FMT_YUV420P) {
        format=MLHQ_FORMAT_YUV420P;
        stride1=w/2;stride2=w/2;
        offset2=y+y/4;length1=y/4;length2=y/4;
        mlhq_copy_rows(data,frame->data[0],frame->linesize[0],(int)w,(int)h);
        mlhq_copy_rows(data+y,frame->data[1],frame->linesize[1],
                       (int)(w/2),(int)(h/2));
        mlhq_copy_rows(data+offset2,frame->data[2],frame->linesize[2],
                       (int)(w/2),(int)(h/2));
    } else {
        format=MLHQ_FORMAT_NV12;
        stride1=w;stride2=0;
        offset2=0;length1=y/2;length2=0;
        mlhq_copy_rows(data,frame->data[0],frame->linesize[0],(int)w,(int)h);
        mlhq_copy_rows(data+y,frame->data[1],frame->linesize[1],
                       (int)w,(int)(h/2));
    }

    mlhq_u32(slot,MLHQ_S_FORMAT,format);
    mlhq_u32(slot,MLHQ_S_WIDTH,w);
    mlhq_u32(slot,MLHQ_S_HEIGHT,h);
    memcpy(slot+MLHQ_S_PTS,&frame->pts,sizeof(frame->pts));
    mlhq_u32(slot,MLHQ_S_STRIDE0,w);
    mlhq_u32(slot,MLHQ_S_STRIDE1,stride1);
    mlhq_u32(slot,MLHQ_S_STRIDE2,stride2);
    mlhq_u32(slot,MLHQ_S_OFFSET0,0);
    mlhq_u32(slot,MLHQ_S_OFFSET1,y);
    mlhq_u32(slot,MLHQ_S_OFFSET2,offset2);
    mlhq_u32(slot,MLHQ_S_LENGTH0,y);
    mlhq_u32(slot,MLHQ_S_LENGTH1,length1);
    mlhq_u32(slot,MLHQ_S_LENGTH2,length2);
    mlhq_u32(slot,MLHQ_S_PAYLOAD,total);
    /* Decoded video buffer pixels already have the server's orientation. */
    mlhq_u32(slot,MLHQ_S_ROTATION,0);

    MemoryBarrier(); // data/metadata visible before even sequence
    InterlockedIncrement(sequence); // odd -> even: committed
    InterlockedExchange((volatile LONG*)(void*)
        (sink->memory+MLHQ_G_LATEST_SLOT),(LONG)index);
    InterlockedIncrement((volatile LONG*)(void*)
        (sink->memory+MLHQ_G_LATEST_SEQ));
}

static DWORD WINAPI
mlhq_worker(LPVOID context) {
    struct sc_mirroleap_hq_sink *sink=context;
    while (InterlockedCompareExchange(&sink->stopping,0,0)==0) {
        WaitForSingleObject(sink->ready,100);
        AVFrame *frame=InterlockedExchangePointer(
            (void *volatile *)&sink->newest,NULL);
        if (frame) {
            if (InterlockedCompareExchange(&sink->stopping,0,0)==0) {
                mlhq_publish(sink,frame);
            }
            av_frame_free(&frame);
        }
    }
    return 0;
}

static void
mlhq_close(struct sc_frame_sink *receiver) {
    struct sc_mirroleap_hq_sink *sink=
        (struct sc_mirroleap_hq_sink*)receiver;
    InterlockedExchange(&sink->stopping,1);
    if (sink->ready) {SetEvent(sink->ready);}
    if (sink->worker) {
        WaitForSingleObject(sink->worker,INFINITE);
        CloseHandle(sink->worker);
        sink->worker=NULL;
    }
    AVFrame *pending=InterlockedExchangePointer(
        (void *volatile *)&sink->newest,NULL);
    if (pending) {av_frame_free(&pending);}
    if (sink->ready) {CloseHandle(sink->ready);sink->ready=NULL;}
    if (sink->memory) {UnmapViewOfFile(sink->memory);sink->memory=NULL;}
    if (sink->mapping) {CloseHandle(sink->mapping);sink->mapping=NULL;}
}

static bool
mlhq_open(struct sc_frame_sink *receiver,const AVCodecContext *ctx,
          const struct sc_stream_session *session) {
    (void)ctx;(void)session;
    struct sc_mirroleap_hq_sink *sink=
        (struct sc_mirroleap_hq_sink*)receiver;
    wchar_t name[96];
    if (!mlhq_mapping_name(name,sizeof(name)/sizeof(name[0]))) {
        return true; // no opt-in mapping: strictly preserve normal mirroring
    }
    sink->mapping=OpenFileMappingW(FILE_MAP_ALL_ACCESS,FALSE,name);
    if (!sink->mapping) {
        LOGW("MirroLeap HQ shared frame receiver not available; using normal video");
        return true;
    }
    sink->memory=MapViewOfFile(sink->mapping,FILE_MAP_ALL_ACCESS,0,0,
                               (SIZE_T)MLHQ_MAPPING_BYTES);
    if (!sink->memory || !mlhq_compatible_mapping(sink->memory)) {
        LOGW("MirroLeap HQ protocol mismatch; using normal video");
        mlhq_close(receiver);
        return true;
    }
    sink->ready=CreateEventW(NULL,FALSE,FALSE,NULL);
    if (!sink->ready) {
        mlhq_close(receiver);
        return true;
    }
    sink->stopping=0;
    sink->worker=CreateThread(NULL,0,mlhq_worker,sink,0,NULL);
    if (!sink->worker) {
        mlhq_close(receiver);
        return true;
    }
    LOGI("MirroLeap experimental HQ frame receiver enabled");
    return true;
}

static bool
mlhq_push(struct sc_frame_sink *receiver,const AVFrame *frame) {
    struct sc_mirroleap_hq_sink *sink=
        (struct sc_mirroleap_hq_sink*)receiver;
    if (!sink->worker || !mlhq_frame_supported(frame)) {return true;}
    // Reference-counted buffer clone only: copying frame pixels is exclusively
    // the worker's job, never a blocking operation on decoder / SDL threads.
    AVFrame *clone=av_frame_clone(frame);
    if (!clone) {return true;}
    AVFrame *stale=InterlockedExchangePointer(
        (void *volatile *)&sink->newest,clone);
    if (stale) {av_frame_free(&stale);} // latest-frame-wins
    SetEvent(sink->ready);
    return true; // HQ failures MUST NOT terminate normal scrcpy playback
}

void
sc_mirroleap_hq_sink_init(struct sc_mirroleap_hq_sink *sink) {
    memset(sink,0,sizeof(*sink));
    static const struct sc_frame_sink_ops ops={
        .open=mlhq_open,
        .close=mlhq_close,
        .push=mlhq_push,
        .push_session=NULL,
    };
    sink->frame_sink.ops=&ops;
}

#endif
