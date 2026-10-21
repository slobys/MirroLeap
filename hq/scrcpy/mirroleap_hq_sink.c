/* Optional decoded-frame shared-memory sink for scrcpy v4.0, Windows.
 * Copyright (c) 2026 MirroLeap contributors. MIT licensed.
 * This is an add-on to the Apache-2.0 scrcpy app, not a replacement server.
 */
#include "mirroleap_hq_sink.h"

#ifdef _WIN32

#include <libavutil/frame.h>
#include <libavutil/pixfmt.h>
#ifdef MLHQ_ENABLE_D3D11
# include <libavutil/hwcontext.h>
#endif
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

// The SDL/FFmpeg pipeline must stay at its normal FPS when the user only
// enabled the HQ option in settings. A decoded-frame copy is authorized
// ONLY while the pointer is inside an active HQ round lens.
static bool
mlhq_consumer_active(const struct sc_mirroleap_hq_sink *sink) {
    return sink->memory &&
        InterlockedCompareExchange((volatile LONG *)(void *)
            (sink->memory+MLHQ_G_CONSUMER_ACTIVE),0,0)==1;
}

static bool
mlhq_frame_supported(const AVFrame *frame) {
    if (!frame || frame->width<2 || frame->height<2 ||
        frame->width>(int)MLHQ_MAX_WIDTH ||
        frame->height>(int)MLHQ_MAX_HEIGHT ||
        (frame->width&1) || (frame->height&1) ||
        (uint64_t)frame->width*(uint64_t)frame->height>MLHQ_MAX_PIXELS) {
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

// The hardware helper calls this YUV writer after the FFmpeg transfer.
static void mlhq_publish(struct sc_mirroleap_hq_sink *sink,
                         const AVFrame *frame);

#ifdef MLHQ_ENABLE_D3D11
// v5 D3D11VA AVFrame.data[0] is a GPU texture (slice in data[1]),
// NOT a CPU YUV plane. The reader's old memcpy path must NEVER touch it.
// FFmpeg hardware transfer is carried out only on our bounded worker, not
// on the decoder's full-rate thread. Unavailable formats fall back to the
// original Windows desktop lens instead of interrupting normal scrcpy video.
static bool
mlhq_hw_frame_supported(const AVFrame *frame) {
    if (!frame || frame->format != AV_PIX_FMT_D3D11 ||
        !frame->data[0] || !frame->hw_frames_ctx ||
        frame->width < 2 || frame->height < 2 ||
        (frame->width & 1) || (frame->height & 1) ||
        (uint64_t)frame->width * (uint64_t)frame->height > MLHQ_MAX_PIXELS ||
        frame->width > (int)MLHQ_MAX_WIDTH ||
        frame->height > (int)MLHQ_MAX_HEIGHT) {
        return false;
    }
    const AVHWFramesContext *frames =
        (const AVHWFramesContext *)frame->hw_frames_ctx->data;
    return frames && frames->sw_format == AV_PIX_FMT_NV12;
}

// The video decoder never blocks on readback. Adapt separate source-pixel
// capture to measured GPU transfer cost; this is not GPU ROI/zero-copy.
static LONG mlhq_gpu_capture_interval_ms(DWORD duration_ms) {
    if (duration_ms<=10u) {return 80;}
    if (duration_ms<=25u) {return 100;}
    return 140;
}
static void
mlhq_publish_hw_or_cpu(struct sc_mirroleap_hq_sink *sink,
                       const AVFrame *frame) {
    if (frame->format != AV_PIX_FMT_D3D11) {
        mlhq_publish(sink, frame);
        return;
    }
    if (!mlhq_hw_frame_supported(frame) ||
        InterlockedCompareExchange(&sink->gpu_readback_disabled,0,0)) {
        return;
    }
    AVFrame *cpu = av_frame_alloc();
    if (!cpu) {return;}
    const DWORD started=GetTickCount();
    const int transferred=av_hwframe_transfer_data(cpu, frame, 0);
    const DWORD readback_ms=(DWORD)(GetTickCount()-started);
    InterlockedExchange(&sink->gpu_interval_ms,
        mlhq_gpu_capture_interval_ms(readback_ms));
    // Hardware D3D11VA reads can starve SDL's own renderer. After TWO slow
    // transfers stop sampling for the session and report a recoverable HQ
    // failure so the old desktop lens remains available without video lag.
    if (readback_ms>45u) {
        if (InterlockedIncrement(&sink->gpu_slow_count)>=2 &&
            !InterlockedExchange(&sink->gpu_readback_disabled,1)) {
            InterlockedExchange((volatile LONG *)(void *)
                (sink->memory+MLHQ_G_GPU_READBACK_SLOW),1);
            LOGW("MirroLeap HQ GPU readback too slow; "
                 "disabling source bypass to protect normal scrcpy video");
        }
    } else {
        InterlockedExchange(&sink->gpu_slow_count,0);
    }
    if (!sink->gpu_readback_disabled && transferred>=0 &&
        mlhq_frame_supported(cpu)) {
        cpu->pts=frame->pts;
        mlhq_publish(sink,cpu);
    }
    av_frame_free(&cpu);
}
#endif

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
    if (!mlhq_consumer_active(sink) || !mlhq_frame_supported(frame)) {return;}
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
    // Do not expose a frame committed after the UI disabled its HQ lens.
    if (!mlhq_consumer_active(sink)) {return;}
    InterlockedExchange((volatile LONG*)(void*)
        (sink->memory+MLHQ_G_LATEST_SLOT),(LONG)index);
    InterlockedIncrement((volatile LONG*)(void*)
        (sink->memory+MLHQ_G_LATEST_SEQ));
}

static DWORD WINAPI
mlhq_worker(LPVOID context) {
    struct sc_mirroleap_hq_sink *sink=context;
    DWORD last_copied_at=0;
    while (InterlockedCompareExchange(&sink->stopping,0,0)==0) {
        WaitForSingleObject(sink->ready,100);
        AVFrame *frame=InterlockedExchangePointer(
            (void *volatile *)&sink->newest,NULL);
        if (frame) {
            if (InterlockedCompareExchange(&sink->stopping,0,0)==0 &&
                mlhq_consumer_active(sink)) {
                const DWORD now=GetTickCount();
                // Never copy more than 20 full YUV frames/second, even when
                // Android sends 60+. The latest frame wins; normal scrcpy
                // decoder, SDL and Android FPS stay entirely independent.
                if (!last_copied_at ||
                    (DWORD)(now-last_copied_at)>=MLHQ_MIN_PUBLISH_MS) {
#ifdef MLHQ_ENABLE_D3D11
                    mlhq_publish_hw_or_cpu(sink,frame);
#else
                    mlhq_publish(sink,frame);
#endif
                    last_copied_at=now ? now : 1;
                }
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

// scrcpy 4.0 expects bool frame_sink.push; scrcpy 5.x expects
// enum sc_sink_result where SC_SINK_OK is ZERO (returning true is an error!).
#ifdef MLHQ_ENABLE_D3D11
# define MLHQ_PUSH_RESULT enum sc_sink_result
# define MLHQ_PUSH_OK SC_SINK_OK
#else
# define MLHQ_PUSH_RESULT bool
# define MLHQ_PUSH_OK true
#endif
static MLHQ_PUSH_RESULT
mlhq_push(struct sc_frame_sink *receiver,const AVFrame *frame) {
    struct sc_mirroleap_hq_sink *sink=
        (struct sc_mirroleap_hq_sink*)receiver;
    if (!sink->worker || !mlhq_consumer_active(sink)) {return MLHQ_PUSH_OK;}
#ifdef MLHQ_ENABLE_D3D11
    if (InterlockedCompareExchange(&sink->gpu_readback_disabled,0,0)) {
        return MLHQ_PUSH_OK;
    }
#endif
#ifdef MLHQ_ENABLE_D3D11
    if (!mlhq_frame_supported(frame) && !mlhq_hw_frame_supported(frame)) {
        return MLHQ_PUSH_OK;
    }
    // D3D11VA uses measured 80/100/140ms intervals; software remains 20fps.
    // No decode or SDL thread does GPU readback or waits for the circle.
    DWORD now=GetTickCount();
    DWORD interval=frame->format==AV_PIX_FMT_D3D11
        ? (DWORD)InterlockedCompareExchange(&sink->gpu_interval_ms,0,0)
        : MLHQ_MIN_PUBLISH_MS;
    DWORD last=(DWORD)InterlockedCompareExchange(&sink->last_enqueued_at,0,0);
    if (last && (DWORD)(now-last)<interval) {return MLHQ_PUSH_OK;}
#else
    if (!mlhq_frame_supported(frame)) {return MLHQ_PUSH_OK;}
#endif
    // Reference-counted GPU/CPU frame only, never a pixel copy here.
    AVFrame *clone=av_frame_clone(frame);
    if (!clone) {return MLHQ_PUSH_OK;}
#ifdef MLHQ_ENABLE_D3D11
    InterlockedExchange(&sink->last_enqueued_at,(LONG)(now ? now : 1u));
#endif
    AVFrame *stale=InterlockedExchangePointer(
        (void *volatile *)&sink->newest,clone);
    if (stale) {av_frame_free(&stale);} // latest-frame-wins
    SetEvent(sink->ready);
    return MLHQ_PUSH_OK; // HQ failures MUST NOT terminate normal scrcpy playback
}

void
sc_mirroleap_hq_sink_init(struct sc_mirroleap_hq_sink *sink) {
    memset(sink,0,sizeof(*sink));
#ifdef MLHQ_ENABLE_D3D11
    sink->gpu_interval_ms=100; // safe startup before first GPU timing
#endif
    static const struct sc_frame_sink_ops ops={
        .open=mlhq_open,
        .close=mlhq_close,
        .push=mlhq_push,
        .push_session=NULL,
    };
    sink->frame_sink.ops=&ops;
}

#endif
