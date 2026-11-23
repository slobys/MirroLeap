// SPDX-License-Identifier: GPL-3.0-only
#include <cassert>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>

#include "Capture/CaptureSession.h"
#include "Protocol/QuickTimeSession.h"
#include "Protocol/QuickTimePacket.h"

int wmain(int argc, wchar_t** argv) {
    using namespace std::chrono_literals;
    using namespace iPhoneMirror::capture::detail;
    using namespace iPhoneMirror::quicktime;

    static_assert(video_silence_action(false, 30s, 0ms) == VideoSilenceAction::None);
    static_assert(video_silence_action(true, 1s, 0ms) == VideoSilenceAction::None);
    static_assert(video_silence_action(true, 3s, 20ms) == VideoSilenceAction::RequestFrame);
    static_assert(video_silence_action(true, 3s, 3s) == VideoSilenceAction::ReopenStream);

    assert(argc == 2);
    const std::filesystem::path folder(argv[1]);
    const auto fixture = [&](const char* name) {
        std::ifstream input(folder / name, std::ios::binary);
        assert(input);
        return std::vector<std::uint8_t>(std::istreambuf_iterator<char>(input), {});
    };
    SessionProtocol session;
    const auto process = [&](std::vector<std::uint8_t> bytes) {
        assert(bytes.size() > 4);
        return session.process(parse_payload(std::span(bytes).subspan(4)));
    };
    assert(session.request_video_sample().empty());
    const auto video_clock = process(fixture("cvrp-request"));
    assert(video_clock.outbound.size() == 2);
    assert(session.request_video_sample().empty());
    const auto video = process(fixture("asyn-feed"));
    assert(video.video_sample && video.state == SessionState::Streaming);
    const auto request = session.request_video_sample();
    assert(request == video_clock.outbound.front());
    assert(session.state() == SessionState::Streaming && session.video_frames() == 1);
    const auto stop = session.stop_messages();
    assert(!stop.empty() && session.request_video_sample().empty());
    std::cout << "QuickTime recovery tests passed: live audio, video NEED, full media silence and teardown\n";
}
