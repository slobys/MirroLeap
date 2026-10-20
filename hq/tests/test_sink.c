/* Runs the actual HQ C packing implementation with test-only Windows and
 * FFmpeg stubs. This is not proof of production Windows cross-compilation.
 * Copyright (c) 2026 MirroLeap contributors (MIT).
 */
#define _WIN32 1
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <wchar.h>

#include "../scrcpy/mirroleap_hq_sink.c"

#define CHECK(cond) do {if (!(cond)) {fprintf(stderr, "HQ C sink invariant failed at line %d: %s\n", __LINE__, #cond);abort();}}while(0)

/* Win32 stubs: no actual hardware/ADB calls. These unused lifecycle
 * functions must link but are never called by this packing unit test. */
DWORD GetEnvironmentVariableA(const char *key,char *buf,DWORD size) {
    (void)key;(void)buf;(void)size;return 0;
}
HANDLE OpenFileMappingW(DWORD a,int b,const wchar_t *c) {
    (void)a;(void)b;(void)c;return NULL;
}
void *MapViewOfFile(HANDLE a,DWORD b,DWORD c,DWORD d,SIZE_T e) {
    (void)a;(void)b;(void)c;(void)d;(void)e;return NULL;
}
HANDLE CreateEventW(void *a,int b,int c,const wchar_t *d) {
    (void)a;(void)b;(void)c;(void)d;return NULL;
}
HANDLE CreateThread(void *a,SIZE_T b,DWORD (WINAPI *c)(LPVOID),LPVOID d,
                    DWORD e,DWORD *f) {
    (void)a;(void)b;(void)c;(void)d;(void)e;(void)f;return NULL;
}
DWORD WaitForSingleObject(HANDLE a,DWORD b) {(void)a;(void)b;return 0;}
int CloseHandle(HANDLE a) {(void)a;return 1;}
int UnmapViewOfFile(const void *a) {(void)a;return 1;}
int SetEvent(HANDLE a) {(void)a;return 1;}
LONG InterlockedIncrement(volatile LONG *a) {return __atomic_add_fetch(a,1,__ATOMIC_SEQ_CST);}
LONG InterlockedExchange(volatile LONG *a,LONG b) {return __atomic_exchange_n(a,b,__ATOMIC_SEQ_CST);}
LONG InterlockedCompareExchange(volatile LONG *a,LONG b,LONG c) {
    __atomic_compare_exchange_n(a,&c,b,0,__ATOMIC_SEQ_CST,__ATOMIC_SEQ_CST);return c;
}
void *InterlockedExchangePointer(void * volatile *a,void *b) {
    return __atomic_exchange_n(a,b,__ATOMIC_SEQ_CST);
}
AVFrame *av_frame_clone(const AVFrame *a) {
    AVFrame *new_frame=malloc(sizeof(*new_frame));
    if (new_frame) memcpy(new_frame,a,sizeof(*new_frame));
    return new_frame;
}
void av_frame_free(AVFrame **a) {if (*a) {free(*a);*a=NULL;}}

static void
initialize(unsigned char *mem) {
    memcpy(mem,"MLHQ",4);
    mlhq_u32(mem,MLHQ_G_VERSION,MLHQ_VERSION);
    mlhq_u32(mem,MLHQ_G_MAPPING_LEN,MLHQ_MAPPING_BYTES);
    mlhq_u32(mem,MLHQ_G_CAPACITY,MLHQ_SLOT_CAPACITY);
    mlhq_u32(mem,MLHQ_G_SLOT_COUNT,MLHQ_SLOT_COUNT);
    mlhq_u32(mem,MLHQ_G_LATEST_SLOT,MLHQ_NO_FRAME);
}

int
main(int argc,char **argv) {
    unsigned char *shared=calloc(MLHQ_MAPPING_BYTES,1);
    CHECK(shared);
    initialize(shared);
    CHECK(mlhq_compatible_mapping(shared));
    struct sc_mirroleap_hq_sink sink={0};
    sc_mirroleap_hq_sink_init(&sink);
    sink.memory=shared;
    uint8_t yy[4*8]={0};
    uint8_t uu[2*4]={0};
    uint8_t vv[2*4]={0};
    for (int row=0;row<4;row++) for (int col=0;col<4;col++)
        yy[row*8+col]=(uint8_t)(row*4+col+1);
    for (int row=0;row<2;row++) for (int col=0;col<2;col++) {
        uu[row*4+col]=(uint8_t)(100+row*2+col);
        vv[row*4+col]=(uint8_t)(150+row*2+col);
    }
    AVFrame frame={0};
    frame.width=4;frame.height=4;frame.format=AV_PIX_FMT_YUV420P;
    frame.data[0]=yy;frame.data[1]=uu;frame.data[2]=vv;
    frame.linesize[0]=8;frame.linesize[1]=4;frame.linesize[2]=4;
    frame.pts=1234567;
    CHECK(mlhq_frame_supported(&frame));
    mlhq_publish(&sink,&frame);
    CHECK(mlhq_read_u32(shared,MLHQ_G_LATEST_SLOT)==0);
    unsigned char *slot=shared+MLHQ_SLOT_BASE(0);
    unsigned char *pixels=shared+MLHQ_DATA_BASE(0);
    CHECK(mlhq_read_u32(slot,MLHQ_S_SEQUENCE)==2);
    CHECK(mlhq_read_u32(slot,MLHQ_S_FORMAT)==MLHQ_FORMAT_YUV420P);
    CHECK(mlhq_read_u32(slot,MLHQ_S_WIDTH)==4);
    CHECK(mlhq_read_u32(slot,MLHQ_S_HEIGHT)==4);
    CHECK(mlhq_read_u32(slot,MLHQ_S_PAYLOAD)==24);
    int64_t pts=0;
    memcpy(&pts,slot+MLHQ_S_PTS,8);
    CHECK(pts==frame.pts);
    for(int i=0;i<16;i++) CHECK(pixels[i]==(unsigned char)(i+1));
    for(int i=0;i<4;i++) CHECK(pixels[16+i]==(unsigned char)(100+i));
    for(int i=0;i<4;i++) CHECK(pixels[20+i]==(unsigned char)(150+i));

    uint8_t interleaved[2*8]={0};
    for(int r=0;r<2;r++) for(int x=0;x<4;x++)
        interleaved[r*8+x]=(uint8_t)(220+r*4+x);
    frame.format=AV_PIX_FMT_NV12;
    frame.data[1]=interleaved;frame.data[2]=NULL;
    frame.linesize[1]=8;frame.linesize[2]=0;
    mlhq_publish(&sink,&frame);
    CHECK(mlhq_read_u32(shared,MLHQ_G_LATEST_SLOT)==1);
    slot=shared+MLHQ_SLOT_BASE(1);
    pixels=shared+MLHQ_DATA_BASE(1);
    CHECK(mlhq_read_u32(slot,MLHQ_S_SEQUENCE)==2);
    CHECK(mlhq_read_u32(slot,MLHQ_S_FORMAT)==MLHQ_FORMAT_NV12);
    CHECK(mlhq_read_u32(slot,MLHQ_S_STRIDE1)==4);
    CHECK(mlhq_read_u32(slot,MLHQ_S_OFFSET1)==16);
    CHECK(mlhq_read_u32(slot,MLHQ_S_OFFSET2)==0);
    CHECK(mlhq_read_u32(slot,MLHQ_S_PAYLOAD)==24);
    for(int i=0;i<8;i++) CHECK(pixels[16+i]==(unsigned char)(220+i));

    const uint32_t committed=mlhq_read_u32(shared,MLHQ_G_LATEST_SEQ);
    frame.format=-1;
    mlhq_publish(&sink,&frame); // unknown/hardware pixel format: ignore
    CHECK(mlhq_read_u32(shared,MLHQ_G_LATEST_SEQ)==committed);
    frame.format=AV_PIX_FMT_NV12;
    frame.width=3;
    mlhq_publish(&sink,&frame); // odd 4:2:0 dimensions: ignore
    CHECK(mlhq_read_u32(shared,MLHQ_G_LATEST_SEQ)==committed);
    frame.width=MLHQ_MAX_WIDTH+2;
    mlhq_publish(&sink,&frame); // bounds check prevents overruns
    CHECK(mlhq_read_u32(shared,MLHQ_G_LATEST_SEQ)==committed);

    /* The sink cannot terminate a normal scrcpy session when HQ is absent. */
    struct sc_mirroleap_hq_sink not_connected={0};
    sc_mirroleap_hq_sink_init(&not_connected);
    CHECK(not_connected.frame_sink.ops->open(
        &not_connected.frame_sink,NULL,NULL));
    CHECK(not_connected.frame_sink.ops->push(
        &not_connected.frame_sink,&frame));
    not_connected.frame_sink.ops->close(&not_connected.frame_sink);

    if (argc>1) {
        /* Exact bytes generated by the *real C writer* can also be parsed
         * by Rust's protocol test, not by a duplicate Rust-only fixture. */
        FILE *out=fopen(argv[1],"wb");
        CHECK(out);
        CHECK(fwrite(shared,1,MLHQ_MAPPING_BYTES,out)==MLHQ_MAPPING_BYTES);
        CHECK(fclose(out)==0);
    }
    free(shared);
    puts("HQ C producer PASS: padded YUV420P/NV12 source packing, 2 slots, "
         "PTS, atomic commit, unsupported formats ignored");
    return 0;
}
