// SPDX-License-Identifier: MIT
// Read one QuickTime chunk without aborting the whole bulk endpoint on timeout.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <winioctl.h>
#include <stddef.h>
#include <stdio.h>
#include "usbi.h"
#include "driver/driver_api.h"

// Both layouts come from the source pinned by build-ios-usb.ps1. The other
// libusb entry points still run in the verified original x64 1.2.6.0 DLL.
_Static_assert(offsetof(struct usb_dev_handle, impl_info) == 40, "libusb handle ABI");
_Static_assert(sizeof(libusb_request) == 24, "libusb request ABI");

enum {
    USB_IO_ERROR = -5,
    USB_INVALID_ARGUMENT = -22,
    USB_TRANSFER_TIMED_OUT = -116,
    READ_GRACE_MS = 3000,
    READ_RETRY_DELAY_MS = 50,
};

typedef int (__cdecl *CloseDevice)(void *);

static INIT_ONCE resolve_once = INIT_ONCE_STATIC_INIT;
static CloseDevice original_close;

static __declspec(thread) struct {
    void *handle;
    int endpoint;
    ULONGLONG first_error;
    BOOL recovering;
    DWORD last_error;
} read_state;

static BOOL CALLBACK resolve_original(PINIT_ONCE once, PVOID parameter, PVOID *context) {
    (void)once;
    (void)parameter;
    (void)context;
    HMODULE self;
    WCHAR path[32768];
    const WCHAR name[] = L"libusb0_original.dll";
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                           GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           (LPCWSTR)&resolve_once, &self)) return FALSE;
    DWORD length = GetModuleFileNameW(self, path, ARRAYSIZE(path));
    if (!length || length >= ARRAYSIZE(path)) return FALSE;
    while (length && path[length - 1] != L'\\') --length;
    if (!length || length + ARRAYSIZE(name) > ARRAYSIZE(path)) return FALSE;
    CopyMemory(path + length, name, sizeof(name));
    HMODULE original = LoadLibraryExW(path, NULL,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!original) return FALSE;
    original_close = (CloseDevice)GetProcAddress(original, "usb_close");
    if (original_close) return TRUE;
    FreeLibrary(original);
    return FALSE;
}

static void log_read_error(const char *event, DWORD error, ULONGLONG elapsed) {
    WCHAR path[MAX_PATH];
    const WCHAR name[] = L"MirroLeap-usb-read.log";
    const DWORD length = GetTempPathW(ARRAYSIZE(path), path);
    if (!length || length > ARRAYSIZE(path) - ARRAYSIZE(name)) return;
    CopyMemory(path + length, name, sizeof(name));
    HANDLE file = CreateFileW(path, FILE_APPEND_DATA,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL, OPEN_ALWAYS,
        FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return;
    SYSTEMTIME time;
    GetLocalTime(&time);
    char line[192];
    const int count = snprintf(line, sizeof(line),
        "%04u-%02u-%02u %02u:%02u:%02u.%03u %s win32=%lu elapsed_ms=%llu\r\n",
        (unsigned)time.wYear, (unsigned)time.wMonth, (unsigned)time.wDay,
        (unsigned)time.wHour, (unsigned)time.wMinute, (unsigned)time.wSecond,
        (unsigned)time.wMilliseconds, event, (unsigned long)error, elapsed);
    DWORD written;
    if (count > 0 && (size_t)count < sizeof(line)) WriteFile(file, line, count, &written, NULL);
    CloseHandle(file);
}

static int read_chunk(HANDLE device, int ep, char *bytes, DWORD size, int timeout) {
    libusb_request request = {0};
    request.endpoint.endpoint = (unsigned int)ep;
    OVERLAPPED io = {0};
    io.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (!io.hEvent) return USB_IO_ERROR;

    DWORD transferred = 0;
    DWORD error = ERROR_SUCCESS;
    if (!DeviceIoControl(device, LIBUSB_IOCTL_INTERRUPT_OR_BULK_READ,
            &request, sizeof(request), bytes, size, &transferred, &io)) {
        error = GetLastError();
        if (error == ERROR_IO_PENDING) {
            const DWORD wait = WaitForSingleObject(io.hEvent, timeout ? (DWORD)timeout : INFINITE);
            const DWORD wait_error = wait == WAIT_TIMEOUT ? ERROR_SEM_TIMEOUT :
                (wait == WAIT_OBJECT_0 ? ERROR_SUCCESS : GetLastError());
            if (wait != WAIT_OBJECT_0) CancelIoEx(device, &io);

            // Drain this request before releasing the event or the caller's
            // buffer. If data arrived during cancellation, deliver it.
            error = GetOverlappedResult(device, &io, &transferred, TRUE) ?
                ERROR_SUCCESS : GetLastError();
            if (error == ERROR_OPERATION_ABORTED && wait_error != ERROR_SUCCESS) {
                error = wait_error;
            }
        }
    }
    CloseHandle(io.hEvent);
    SetLastError(error);
    if (error == ERROR_SUCCESS) return (int)transferred;
    if (error == ERROR_SEM_TIMEOUT) return USB_TRANSFER_TIMED_OUT;
    if (error == ERROR_INVALID_PARAMETER) return USB_INVALID_ARGUMENT;
    return USB_IO_ERROR;
}

int __cdecl mirroleap_bulk_read(usb_dev_handle *handle, int ep, char *bytes, int size, int timeout) {
    if (!handle || !bytes || size <= 0 || timeout < 0 || !(ep & 0x80) || ep > 0xff ||
        handle->config <= 0 || handle->interface < 0) {
        SetLastError(ERROR_INVALID_PARAMETER);
        return USB_INVALID_ARGUMENT;
    }
    // The legacy synchronous DLL accumulates multiple 64 KiB requests. If
    // a later request times out it returns an error and loses earlier bytes.
    // Return each completed chunk immediately so QuickTime can reassemble it.
    const DWORD chunk = (DWORD)(size > LIBUSB_MAX_READ_WRITE ? LIBUSB_MAX_READ_WRITE : size);
    const int result = read_chunk(handle->impl_info, ep, bytes, chunk, timeout);
    const DWORD error = GetLastError();
    const BOOL same_pipe = read_state.handle == handle && read_state.endpoint == ep;
    if (result > 0) {
        if (same_pipe && read_state.recovering) {
            log_read_error("read_resumed", read_state.last_error,
                GetTickCount64() - read_state.first_error);
        }
        read_state.handle = handle;
        read_state.endpoint = ep;
        read_state.recovering = FALSE;
    } else if (result == USB_IO_ERROR && same_pipe) {
        const ULONGLONG now = GetTickCount64();
        if (!read_state.recovering) {
            read_state.first_error = now;
            read_state.recovering = TRUE;
            read_state.last_error = ERROR_SUCCESS;
        }
        if (error != read_state.last_error) {
            log_read_error("read_failed", error, now - read_state.first_error);
            read_state.last_error = error;
        }
        if (error != ERROR_DEVICE_NOT_CONNECTED && error != ERROR_NO_SUCH_DEVICE &&
            error != ERROR_INVALID_HANDLE && now - read_state.first_error < READ_GRACE_MS) {
            Sleep(READ_RETRY_DELAY_MS);
            SetLastError(ERROR_SEM_TIMEOUT);
            return USB_TRANSFER_TIMED_OUT;
        }
    }
    SetLastError(error);
    return result;
}

int __cdecl mirroleap_usb_close(usb_dev_handle *handle) {
    if (!InitOnceExecuteOnce(&resolve_once, resolve_original, NULL, NULL)) return USB_IO_ERROR;
    if (read_state.handle == handle) read_state.handle = NULL;
    return original_close(handle);
}
