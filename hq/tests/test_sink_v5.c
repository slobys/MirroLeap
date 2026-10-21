/* Tests the actual scrcpy 5.0.1 HQ source-frame C sink WITH the
 * MLHQ_ENABLE_D3D11 path compiled. Uses fake FFmpeg D3D11 GPU frames;
 * validates bounded source-frame references and worker readback plumbing.
 * This is NOT a benchmark of hardware D3D11 or a real Android video stream.
 */
#define MLHQ_ENABLE_D3D11 1
#define main original_yuv_test_main
#include "test_sink.c"
#undef main

static unsigned hw_transfers;
static bool slow_gpu_readback;
static uint8_t sample_y[4*8];
static uint8_t sample_uv[2*8];
AVFrame *av_frame_alloc(void) {
    return calloc(1,sizeof(AVFrame));
}
int av_hwframe_transfer_data(AVFrame *dst,const AVFrame *src,int flags) {
    (void)flags;
    hw_transfers++;
    if (slow_gpu_readback) mock_tick_ms+=60;
    if (src->format!=AV_PIX_FMT_D3D11 || !src->hw_frames_ctx) {return -1;}
    dst->format=AV_PIX_FMT_NV12;
    dst->width=src->width;
    dst->height=src->height;
    dst->data[0]=sample_y;
    dst->data[1]=sample_uv;
    dst->linesize[0]=8;
    dst->linesize[1]=8;
    return 0;
}
int main(int argc,char **argv) {
    /* The existing full v2 software protocol tests must still pass unchanged. */
    CHECK(original_yuv_test_main(argc,argv)==0);
    unsigned char *shared=calloc(MLHQ_MAPPING_BYTES,1);
    CHECK(shared);
    initialize(shared);
    struct sc_mirroleap_hq_sink sink={0};
    sc_mirroleap_hq_sink_init(&sink);
    sink.memory=shared;
    sink.worker=(HANDLE)(uintptr_t)1; // deterministic handoff, no OS thread
    AVHWFramesContext hardware={0};
    hardware.sw_format=AV_PIX_FMT_NV12;
    AVBufferRef hw_ref={0};
    hw_ref.data=(uint8_t *)&hardware;
    AVFrame frame={0};
    frame.format=AV_PIX_FMT_D3D11;
    frame.hw_frames_ctx=&hw_ref;
    frame.data[0]=(uint8_t *)(uintptr_t)1; // opaque GPU texture pointer
    frame.width=4;frame.height=4;frame.pts=7654321;
    for (int i=0;i<16;i++) sample_y[(i/4)*8+i%4]=(uint8_t)(i+3);
    for (int i=0;i<8;i++) sample_uv[(i/4)*8+i%4]=(uint8_t)(100+i);
    CHECK(mlhq_hw_frame_supported(&frame));
    const unsigned initial_clones=clone_count;
    CHECK(sink.frame_sink.ops->push(&sink.frame_sink,&frame)==SC_SINK_OK);
    CHECK(clone_count==initial_clones+1);
    /* The GPU frame is accepted only as an AVFrame ref; CPU readback happens
     * independently in the worker and not on the video decoder thread. */
    CHECK(hw_transfers==0);
    CHECK(sink.frame_sink.ops->push(&sink.frame_sink,&frame)==SC_SINK_OK);
    CHECK(clone_count==initial_clones+1); // same GetTickCount throttled
    AVFrame *pending=InterlockedExchangePointer(
        (void *volatile *)&sink.newest,NULL);
    CHECK(pending!=NULL);
    mlhq_publish_hw_or_cpu(&sink,pending);
    CHECK(hw_transfers==1);
    CHECK(mlhq_read_u32(shared,MLHQ_G_LATEST_SLOT)==0);
    unsigned char *slot=shared+MLHQ_SLOT_BASE(0);
    CHECK(mlhq_read_u32(slot,MLHQ_S_FORMAT)==MLHQ_FORMAT_NV12);
    CHECK(mlhq_read_u32(slot,MLHQ_S_WIDTH)==4);
    CHECK(mlhq_read_u32(slot,MLHQ_S_HEIGHT)==4);
    int64_t pts=0;
    memcpy(&pts,slot+MLHQ_S_PTS,sizeof(pts));
    CHECK(pts==frame.pts);
    av_frame_free(&pending);
    // Unsupported GPU sw_format must not trigger transfer/publish.
    hardware.sw_format=AV_PIX_FMT_YUV420P; // scrcpy 5 D3D11 interop expects NV12
    CHECK(!mlhq_hw_frame_supported(&frame));
    mlhq_publish_hw_or_cpu(&sink,&frame);
    CHECK(hw_transfers==1);
    // Two deliberately slow hardware transfers must stop the source bypass
    // and set an IPC status that the Rust viewer can use to fall back to its
    // smooth desktop lens. Normal scrcpy decoding may continue unhindered.
    hardware.sw_format=AV_PIX_FMT_NV12;
    slow_gpu_readback=true;
    mlhq_publish_hw_or_cpu(&sink,&frame);
    CHECK(hw_transfers==2);
    CHECK(!sink.gpu_readback_disabled);
    mlhq_publish_hw_or_cpu(&sink,&frame);
    CHECK(hw_transfers==3);
    CHECK(sink.gpu_readback_disabled);
    CHECK(mlhq_read_u32(shared,MLHQ_G_GPU_READBACK_SLOW)==1);
    const unsigned clones_when_disabled=clone_count;
    CHECK(sink.frame_sink.ops->push(&sink.frame_sink,&frame)==SC_SINK_OK);
    CHECK(clone_count==clones_when_disabled);
    // All further GPU downloads are disabled for this session.
    mlhq_publish_hw_or_cpu(&sink,&frame);
    CHECK(hw_transfers==3);
    // Disabling the circular lens must stop all frame refs and transfers.
    mlhq_u32(shared,MLHQ_G_CONSUMER_ACTIVE,0);
    CHECK(sink.frame_sink.ops->push(&sink.frame_sink,&frame)==SC_SINK_OK);
    CHECK(clone_count==initial_clones+1);
    CHECK(hw_transfers==3);
    // No connected map = ordinary scrcpy still accepts frames.
    sink.memory=NULL;
    CHECK(sink.frame_sink.ops->push(&sink.frame_sink,&frame)==SC_SINK_OK);
    free(shared);
    puts("HQ v5 D3D11VA frame source tests PASS (stub GPU surface)");
    return 0;
}
