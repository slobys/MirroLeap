// SPDX-License-Identifier: GPL-3.0-only
// MirroLeap's automatic USB preview, using the iPhoneMirror v1.8.3 Core API.
#include <windows.h>
#include <windowsx.h>
#include <shellapi.h>
#include <shlobj.h>
#include <objbase.h>
#include <algorithm>
#include <cstdio>
#include <cwctype>
#include <filesystem>
#include <memory>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>
#include "CoreApi.h"
#include "hardware_lease.h"
#include "driver_backend.h"

using namespace iPhoneMirror;
constexpr wchar_t PreviewClass[] = L"MirroLeap.UsbPreview.v1";
constexpr UINT_PTR PollTimer = 1;

void report(const char* status) {
    static std::string last;
    if (last == status) return;
    last = status;
    std::puts(status);
    std::fflush(stdout);
}

std::filesystem::path runtime_directory() {
    wchar_t own_file[32768]{};
    const auto length = GetModuleFileNameW(nullptr, own_file, ARRAYSIZE(own_file));
    if (!length || length >= ARRAYSIZE(own_file)) throw std::runtime_error("module path");
    return std::filesystem::path(own_file).parent_path();
}

struct CaptureCore {
    HMODULE original_usb{};
    HMODULE usb_adapter{};
    HMODULE module{};
    bool initialized{};
#define API(name) decltype(&im_##name) name{}
    API(initialize); API(shutdown); API(api_version); API(refresh_devices_ex);
    API(get_environment); API(is_libusb0_device_available); API(session_create);
    API(session_attach_preview); API(session_get_status); API(session_stop); API(session_destroy);
#undef API

    void load() {
        const auto directory = runtime_directory();
        constexpr auto search = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
        // Resolve the forwarding DLL's sibling exports with the same restricted
        // search policy even when the core resolves libusb functions lazily.
        original_usb = LoadLibraryExW((directory / L"libusb0_original.dll").c_str(), nullptr, search);
        if (!original_usb) throw std::runtime_error("original USB DLL");
        usb_adapter = LoadLibraryExW((directory / L"libusb0.dll").c_str(), nullptr, search);
        if (!usb_adapter || !GetProcAddress(usb_adapter, "usb_bulk_read") ||
            GetProcAddress(usb_adapter, "usb_get_version") != GetProcAddress(original_usb, "usb_get_version"))
            throw std::runtime_error("USB read adapter");
        module = LoadLibraryExW((directory / L"iPhoneMirror.Core.dll").c_str(), nullptr, search);
        if (!module) throw std::runtime_error("core DLL");
#define LOAD(name) name = reinterpret_cast<decltype(name)>(GetProcAddress(module, "im_" #name)); \
    if (!name) throw std::runtime_error("core API")
        LOAD(initialize); LOAD(shutdown); LOAD(api_version); LOAD(refresh_devices_ex);
        LOAD(get_environment); LOAD(is_libusb0_device_available); LOAD(session_create);
        LOAD(session_attach_preview); LOAD(session_get_status); LOAD(session_stop); LOAD(session_destroy);
#undef LOAD
        if (api_version() != ApiVersion || initialize() != 0) throw std::runtime_error("core initialization");
        initialized = true;
    }

    ~CaptureCore() {
        if (initialized) shutdown();
        if (module) FreeLibrary(module);
        if (usb_adapter) FreeLibrary(usb_adapter);
        if (original_usb) FreeLibrary(original_usb);
    }
};

std::optional<std::vector<DeviceInfo>> usb_devices(const CaptureCore& core, bool refresh_metadata) {
    std::vector<DeviceInfo> devices(128);
    auto count = static_cast<std::uint32_t>(devices.size());
    if (core.refresh_devices_ex(devices.data(), &count, refresh_metadata ? 1 : 0) != 0 ||
        count > devices.size()) return std::nullopt;
    devices.resize(count);
    devices.erase(std::remove_if(devices.begin(), devices.end(), [](const DeviceInfo& device) {
        return !device.usb_connected || !device.udid[0];
    }), devices.end());
    return devices;
}

struct UsbReadiness {
    const DeviceInfo* paired{};
    std::string_view status{};
};

UsbReadiness usb_readiness(const std::optional<std::vector<DeviceInfo>>& devices,
    std::wstring_view expected_device = {}) {
    // Windows can detect a USB device before Apple exposes it through usbmux.
    // A failed or empty enumeration gives no information about authorization.
    if (!devices || devices->empty()) return {nullptr, "detecting"};
    const auto matches = [&](const auto& device) {
        return expected_device.empty() || expected_device == device.udid;
    };
    const auto paired = std::find_if(devices->begin(), devices->end(), [&](const auto& device) {
        return matches(device) && device.pair_record_present && device.lockdown_accessible;
    });
    if (paired != devices->end()) return {&*paired, {}};
    const auto paired_without_metadata = std::find_if(devices->begin(), devices->end(), [&](const auto& device) {
        return matches(device) && device.pair_record_present;
    });
    if (paired_without_metadata != devices->end()) return {&*paired_without_metadata, {}};
    return {nullptr, "authorization_pending"};
}

bool recoverable_usb_failure(const CaptureStatus& status) {
    if (status.state != CaptureState::Error) return false;
    // The pinned libusb0 runtime can report its bulk I/O error only in the
    // diagnostic text, even though it sets the typed failure kind and stage.
    const std::wstring_view message(status.message);
    return status.failure_kind == CaptureFailureKind::Driver &&
        status.failure_stage == CaptureFailureStage::VideoStream &&
        message.find(L"could not confirm that the Apple USB device returned") == std::wstring_view::npos &&
        (status.error_code == -5 || message.find(L"libusb0 error -5") != std::wstring_view::npos);
}

struct UsbRecovery {
    static constexpr unsigned MaxAttempts = 2;
    static constexpr ULONGLONG DiscoveryTimeoutMs = 15000;
    static constexpr ULONGLONG StablePlaybackMs = 30000;
    unsigned attempts{};
    ULONGLONG deadline{};

    bool pending() const { return deadline != 0; }
    bool can_retry() const { return attempts < MaxAttempts; }
    void schedule(ULONGLONG now) { deadline = now + DiscoveryTimeoutMs; }
    bool expired(ULONGLONG now) const { return pending() && now >= deadline; }
    void begin_attempt() {
        if (pending()) ++attempts;
        deadline = 0;
    }
};

const char* capture_readiness(const DeviceInfo& device, int probe, bool driver_ready) {
    if (probe != 0) return "detecting";
    if (driver_ready) return "ready";
    return device.lockdown_accessible ? "capture_driver" : "unlock";
}

bool test_usb_readiness() {
    DeviceInfo paired{};
    paired.pair_record_present = 1;
    DeviceInfo paired_accessible = paired;
    paired_accessible.lockdown_accessible = 1;
    DeviceInfo without_pair_record{};
    without_pair_record.lockdown_accessible = 1;
    const std::optional<std::vector<DeviceInfo>> records{
        std::vector<DeviceInfo>{without_pair_record, paired, paired_accessible}};
    const std::optional<std::vector<DeviceInfo>> paired_only{std::vector<DeviceInfo>{paired}};
    return usb_readiness(std::nullopt).status == "detecting" &&
        usb_readiness(std::vector<DeviceInfo>{}).status == "detecting" &&
        usb_readiness(std::vector<DeviceInfo>{DeviceInfo{}}).status == "authorization_pending" &&
        usb_readiness(std::vector<DeviceInfo>{without_pair_record}).status == "authorization_pending" &&
        usb_readiness(records).paired == &records->back() &&
        usb_readiness(paired_only).paired == &paired_only->front() &&
        std::string_view(capture_readiness(paired, 0, true)) == "ready" &&
        std::string_view(capture_readiness(paired, 0, false)) == "unlock" &&
        std::string_view(capture_readiness(paired_accessible, 0, false)) == "capture_driver" &&
        std::string_view(capture_readiness(paired_accessible, -10, false)) == "detecting";
}

bool test_usb_recovery() {
    CaptureStatus interrupted{};
    interrupted.state = CaptureState::Error;
    interrupted.failure_kind = CaptureFailureKind::Driver;
    interrupted.failure_stage = CaptureFailureStage::VideoStream;
    wcscpy_s(interrupted.message, L"QuickTime bulk read: libusb0 error -5");
    if (!recoverable_usb_failure(interrupted)) return false;
    interrupted.failure_stage = CaptureFailureStage::Decoder;
    if (recoverable_usb_failure(interrupted)) return false;
    interrupted.failure_stage = CaptureFailureStage::VideoStream;
    wcscat_s(interrupted.message, L"; could not confirm that the Apple USB device returned");
    if (recoverable_usb_failure(interrupted)) return false;

    DeviceInfo other{};
    wcscpy_s(other.udid, L"other-device");
    other.pair_record_present = other.lockdown_accessible = 1;
    const std::optional<std::vector<DeviceInfo>> devices{std::vector<DeviceInfo>{other}};
    if (usb_readiness(devices, L"original-device").paired || !usb_readiness(devices, L"other-device").paired)
        return false;

    UsbRecovery recovery;
    for (unsigned attempt = 0; attempt < UsbRecovery::MaxAttempts; ++attempt) {
        if (!recovery.can_retry()) return false;
        recovery.schedule(1000);
        if (!recovery.pending() || recovery.expired(2000) ||
            !recovery.expired(1000 + UsbRecovery::DiscoveryTimeoutMs)) return false;
        recovery.begin_attempt();
    }
    return !recovery.pending() && !recovery.can_retry();
}

std::wstring normalized_serial(std::wstring value) {
    value.erase(std::remove(value.begin(), value.end(), L'-'), value.end());
    for (auto& c : value) c = static_cast<wchar_t>(std::towupper(c));
    return value;
}

int install_driver(const CaptureCore& core) {
    auto detected = usb_devices(core, true);
    if (!detected) { report("detecting"); return 1; }
    auto& trusted = *detected;
    trusted.erase(std::remove_if(trusted.begin(), trusted.end(), [](const DeviceInfo& device) {
        return !device.pair_record_present || !device.lockdown_accessible;
    }), trusted.end());
    if (trusted.size() != 1) { report("one_device"); return 1; }
    std::int32_t ready{};
    if (core.is_libusb0_device_available(trusted.front().udid, &ready) == 0 && ready) return 0;
    const auto serial = normalized_serial(trusted.front().udid);
    const auto devices = usbmirror::driver::devices();
    const auto matches = [&](const usbmirror::driver::Device& device) {
        return device.present && device.healthy && usbmirror::driver::equal(device.service, L"usbccgp") &&
            normalized_serial(device.id.substr(device.id.rfind(L'\\') + 1)) == serial;
    };
    const auto found = std::find_if(devices.begin(), devices.end(), matches);
    if (found == devices.end() || std::count_if(devices.begin(), devices.end(), matches) != 1) {
        report("no_driver_target");
        return 1;
    }
    const auto executable = runtime_directory() / L"USBMirror.Driver.exe";
    const auto args = L"--apply install \"" + found->id + L"\"";
    SHELLEXECUTEINFOW request{sizeof(request)};
    request.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    request.lpVerb = L"runas";
    request.lpFile = executable.c_str();
    request.lpParameters = args.c_str();
    request.nShow = SW_HIDE;
    if (!ShellExecuteExW(&request) || !request.hProcess) { report("driver_cancelled"); return 1; }
    usbmirror::driver::Handle operation(request.hProcess);
    DWORD result{};
    if (WaitForSingleObject(operation.value, INFINITE) != WAIT_OBJECT_0 ||
        !GetExitCodeProcess(operation.value, &result) || result != 0) {
        report("driver_failed");
        return 1;
    }
    return 0;
}

struct UsbPreview {
    CaptureCore core;
    HANDLE parent{};
    SessionHandle session{};
    std::unique_ptr<usbmirror::HardwareLease> lease;
    HWND window{};
    HANDLE commands{GetStdHandle(STD_INPUT_HANDLE)};
    bool requested{}, fullscreen{}, interrupted{};
    RECT normal_bounds{};
    ULONGLONG next_discovery{}, next_attempt{}, next_metadata_refresh{}, activated{}, stream_started{};
    unsigned size_width{}, size_height{}, status_failures{};
    std::wstring selected_device;
    UsbRecovery recovery;

    void close_session() {
        ShowWindow(window, SW_HIDE);
        if (fullscreen) toggle_fullscreen();
        size_width = size_height = 0;
        stream_started = 0;
        status_failures = 0;
        if (session) {
            core.session_stop(session);
            core.session_destroy(session);
            session = 0;
        }
        lease.reset();
    }

    void stop() {
        close_session();
        requested = interrupted = false;
        selected_device.clear();
        recovery = {};
    }

    void capture_failed(bool retry) {
        const bool was_mirroring = stream_started || recovery.pending() || recovery.attempts;
        if (was_mirroring && retry && recovery.can_retry()) {
            report("reconnecting");
            close_session();
            const auto restored_at = GetTickCount64();
            recovery.schedule(restored_at);
            next_discovery = next_attempt = restored_at + 1500;
            return;
        }
        stop();
        interrupted = was_mirroring;
        report(interrupted ? "interrupted" : "unavailable");
        next_discovery = next_attempt = GetTickCount64() + 2000;
    }

    void toggle_fullscreen() {
        RECT rect = normal_bounds;
        if (!fullscreen) {
            MONITORINFO monitor{sizeof(monitor)};
            if (!GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &monitor)) return;
            GetWindowRect(window, &normal_bounds);
            rect = monitor.rcMonitor;
        }
        SetWindowPos(window, nullptr, rect.left, rect.top, rect.right-rect.left, rect.bottom-rect.top,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED);
        fullscreen = !fullscreen;
    }

    bool read_controls() {
        DWORD queued{};
        if (!PeekNamedPipe(commands, nullptr, 0, nullptr, &queued, nullptr)) return false;
        char input[16]{};
        DWORD read{};
        if (queued && !ReadFile(commands, input, std::min<DWORD>(queued, sizeof(input)), &read, nullptr)) return false;
        for (DWORD index = 0; index < read; ++index) {
            if (input[index] == 'S' && !session && !recovery.pending()) {
                interrupted = false;
                requested = true;
                next_discovery = next_attempt = 0;
            } else if (input[index] == 'T') {
                report("stopping");
                stop();
                next_discovery = 0;
            }
        }
        return true;
    }

    void fit(unsigned width, unsigned height) {
        if (fullscreen || (size_width == width && size_height == height)) return;
        const auto monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        MONITORINFO info{sizeof(info)};
        if (!GetMonitorInfoW(monitor, &info)) return;
        const auto& work = info.rcWork;
        const auto limit = std::max(120L, std::min(work.right-work.left, work.bottom-work.top)-100);
        const auto scale = (0.85 * limit) / std::max(width, height);
        RECT bounds{0, 0, static_cast<LONG>(width * scale), static_cast<LONG>(height * scale)};
        RECT old{};
        GetWindowRect(window, &old);
        const auto outer_width = bounds.right - bounds.left;
        const auto outer_height = bounds.bottom - bounds.top;
        const auto cx = size_width ? (old.left+old.right)/2 : (work.left+work.right)/2;
        const auto cy = size_height ? (old.top+old.bottom)/2 : (work.top+work.bottom)/2;
        SetWindowPos(window, nullptr,
            std::clamp(cx-outer_width/2, work.left, std::max(work.left, work.right-outer_width)),
            std::clamp(cy-outer_height/2, work.top, std::max(work.top, work.bottom-outer_height)),
            outer_width, outer_height, SWP_NOZORDER | SWP_NOACTIVATE);
        if (!size_width) ShowWindow(window, SW_SHOWNOACTIVATE);
        size_width = width;
        size_height = height;
        report(("streaming " + std::to_string(width) + " " + std::to_string(height)).c_str());
    }

    void poll() {
        if (WaitForSingleObject(parent, 0) != WAIT_TIMEOUT || !read_controls()) {
            DestroyWindow(window);
            return;
        }
        const auto now = GetTickCount64();
        if (session) {
            CaptureStatus status{};
            status.struct_size = sizeof(status);
            const auto ok = core.session_get_status(session, &status) == 0;
            if (!ok) {
                if (++status_failures >= 3) capture_failed(false);
                return;
            }
            status_failures = 0;
            if (status.state == CaptureState::Streaming && status.width && status.height) {
                if (!stream_started) stream_started = now;
                if (now - stream_started >= UsbRecovery::StablePlaybackMs) recovery.attempts = 0;
                fit(status.width, status.height);
                return;
            }
            if (status.state != CaptureState::Error && status.state != CaptureState::Stopped &&
                (stream_started || status.video_frames || now-activated < 20000)) return;
            capture_failed(recoverable_usb_failure(status));
            return;
        }
        if (recovery.expired(now)) { capture_failed(false); return; }
        const bool recovering = recovery.pending();
        if (now < next_discovery) return;
        next_discovery = now + 2500;
        EnvironmentInfo environment{};
        environment.struct_size = sizeof(environment);
        if (core.get_environment(&environment) != 0) {
            if (!recovering) report("unavailable");
            return;
        }
        if (!environment.apple_mobile_device_service_running) {
            if (!recovering) report("apple_driver");
            return;
        }
        if (!environment.physical_apple_usb_devices) {
            next_metadata_refresh = 0;
            if (!recovering) { requested = interrupted = false; report("waiting"); }
            return;
        }
        // Keep checking USB presence, but avoid opening a new Lockdown
        // tunnel every 2.5 seconds just to repaint the idle status.
        const bool refresh_metadata = requested || now >= next_metadata_refresh;
        const auto devices = usb_devices(core, refresh_metadata);
        const auto readiness = usb_readiness(devices, recovering ? std::wstring_view(selected_device) : L"");
        if (refresh_metadata) {
            next_metadata_refresh = now +
                (readiness.paired && readiness.paired->lockdown_accessible ? 30000 : 5000);
        }
        if (!readiness.paired) {
            if (!recovering) { requested = false; report(readiness.status.data()); }
            return;
        }
        const auto& ready = *readiness.paired;
        if (now < next_attempt) return;
        next_attempt = now + 5000;
        lease = usbmirror::HardwareLease::acquire();
        if (!lease) { if (!recovering) report("busy"); return; }
        std::int32_t filter{};
        const auto probe = core.is_libusb0_device_available(ready.udid, &filter);
        if (const std::string_view state = capture_readiness(ready, probe, filter != 0); state != "ready") {
            if (!recovering) report(state.data());
            lease.reset();
            return;
        }
        if (!requested) {
            report(interrupted ? "interrupted" : "ready");
            lease.reset();
            return;
        }
        CaptureOptions options{};
        options.struct_size = sizeof(options);
        options.api_version = ApiVersion;
        options.target_fps = 60;
        options.play_audio = 1;
        options.audio_volume = 1.0F;
        selected_device = ready.udid;
        recovery.begin_attempt();
        report(recovering ? "reconnecting" : "connecting");
        if (core.session_create(ready.udid, &options, &session) != 0 || !session ||
            core.session_attach_preview(session, window) != 0) {
            capture_failed(recovering);
            return;
        }
        activated = GetTickCount64();
    }

    ~UsbPreview() { if (session) stop(); if (parent) CloseHandle(parent); }
};

LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wp, LPARAM lp) {
    auto* app = reinterpret_cast<UsbPreview*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        app = static_cast<UsbPreview*>(reinterpret_cast<CREATESTRUCTW*>(lp)->lpCreateParams);
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(app));
    }
    if (app && message == WM_TIMER && wp == PollTimer) { app->poll(); return 0; }
    if (message == WM_NCCALCSIZE) return 0;
    if (app && message == WM_NCHITTEST && !app->fullscreen) {
        RECT rect{};
        GetWindowRect(window, &rect);
        const auto inset = MulDiv(7, GetDpiForWindow(window), 96);
        const auto x = GET_X_LPARAM(lp), y = GET_Y_LPARAM(lp);
        const bool left = x < rect.left + inset, right = x >= rect.right - inset;
        const bool top = y < rect.top + inset, bottom = y >= rect.bottom - inset;
        if (top) return left ? HTTOPLEFT : right ? HTTOPRIGHT : HTTOP;
        if (bottom) return left ? HTBOTTOMLEFT : right ? HTBOTTOMRIGHT : HTBOTTOM;
        if (left || right) return left ? HTLEFT : HTRIGHT;
        return HTCLIENT;
    }
    if (app && message == WM_CLOSE) {
        app->stop();
        app->next_discovery = 0;
        report("ready");
        return 0;
    }
    if (app && message == WM_KEYDOWN && (wp == VK_F11 || (wp == VK_ESCAPE && app->fullscreen)) && app->session) {
        app->toggle_fullscreen();
        return 0;
    }
    if (app && message == WM_APP) { DestroyWindow(window); return 0; }
    if (message == WM_DESTROY) {
        KillTimer(window, PollTimer);
        if (app) app->stop();
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(window, message, wp, lp);
}

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE, PWSTR, int) {
    SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32);
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    const auto com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    int result = 1;
    try {
        int count{};
        auto arguments = CommandLineToArgvW(GetCommandLineW(), &count);
        const std::wstring mode = arguments && count == 2 ? arguments[1] : L"";
        if (arguments) LocalFree(arguments);
        UsbPreview app;
        if (mode == L"--self-test-usb") {
            if (test_usb_readiness() && test_usb_recovery()) {
                report("USB readiness and recovery tests passed");
                result = 0;
            }
        } else if (mode == L"--install-driver") {
            app.core.load();
            result = install_driver(app.core);
        } else if (mode == L"--probe") {
            app.core.load();
            EnvironmentInfo environment{};
            environment.struct_size = sizeof(environment);
            if (app.core.get_environment(&environment) != 0) throw std::runtime_error("environment");
            report(environment.apple_mobile_device_service_running ? "ready" : "apple_driver");
            result = 0;
        } else if (!mode.empty() && mode.find_first_not_of(L"0123456789") == std::wstring::npos) {
            app.parent = OpenProcess(SYNCHRONIZE, FALSE, std::stoul(mode));
            if (!app.parent) throw std::runtime_error("owner");
            app.core.load();
            WNDCLASSW klass{};
            klass.hInstance = instance;
            klass.lpfnWndProc = window_proc;
            klass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
            klass.hbrBackground = static_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
            klass.lpszClassName = PreviewClass;
            if (!RegisterClassW(&klass)) throw std::runtime_error("preview class");
            app.window = CreateWindowExW(0, PreviewClass, L"MirroLeap · iPhone / iPad USB",
                WS_POPUP | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
                CW_USEDEFAULT, CW_USEDEFAULT, 400, 780,
                nullptr, nullptr, instance, &app);
            if (!app.window || !SetTimer(app.window, PollTimer, 300, nullptr)) throw std::runtime_error("preview");
            report("waiting");
            MSG msg{};
            while (GetMessageW(&msg, nullptr, 0, 0) > 0) { TranslateMessage(&msg); DispatchMessageW(&msg); }
            result = 0;
        }
    } catch (const std::exception&) { report("unavailable"); }
    if (SUCCEEDED(com)) CoUninitialize();
    return result;
}
