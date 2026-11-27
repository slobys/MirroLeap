// SPDX-License-Identifier: MIT
// Poll one QuickTime USB read without canceling it at software polling deadlines.
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
typedef int (__cdecl *ReleaseInterface)(usb_dev_handle *, int);
typedef int (__cdecl *ControlMessage)(usb_dev_handle *, int, int, int, int, char *, int, int);

static INIT_ONCE resolve_once = INIT_ONCE_STATIC_INIT;
static CloseDevice original_close;
static ReleaseInterface original_release;
static ControlMessage original_control;

typedef struct PendingRead {
    usb_dev_handle *owner;
    HANDLE device;
    int endpoint;
    SRWLOCK lock;
    OVERLAPPED io;
    BOOL pending;
    DWORD offset;
    DWORD count;
    struct PendingRead *next;
    char bytes[LIBUSB_MAX_READ_WRITE];
} PendingRead;

static SRWLOCK readers_lock = SRWLOCK_INIT;
static PendingRead *readers;

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
    original_release = (ReleaseInterface)GetProcAddress(original, "usb_release_interface");
    original_control = (ControlMessage)GetProcAddress(original, "usb_control_msg");
    if (original_close && original_release && original_control) return TRUE;
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

static PendingRead *find_reader(usb_dev_handle *owner, int endpoint) {
    for (PendingRead *reader = readers; reader; reader = reader->next) {
        if (reader->owner == owner && reader->endpoint == endpoint) return reader;
    }
    return NULL;
}

// Return a reader with the registry held shared so teardown cannot free it.
static PendingRead *lock_reader(usb_dev_handle *owner, int endpoint) {
    AcquireSRWLockShared(&readers_lock);
    PendingRead *reader = find_reader(owner, endpoint);
    if (reader) return reader;
    ReleaseSRWLockShared(&readers_lock);

    AcquireSRWLockExclusive(&readers_lock);
    reader = find_reader(owner, endpoint);
    if (!reader) {
        reader = HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof(*reader));
        if (reader) {
            reader->io.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
            if (reader->io.hEvent) {
                reader->owner = owner;
                reader->device = owner->impl_info;
                reader->endpoint = endpoint;
                InitializeSRWLock(&reader->lock);
                reader->next = readers;
                readers = reader;
            } else {
                HeapFree(GetProcessHeap(), 0, reader);
                reader = NULL;
            }
        }
    }
    ReleaseSRWLockExclusive(&readers_lock);
    if (!reader) return NULL;

    AcquireSRWLockShared(&readers_lock);
    reader = find_reader(owner, endpoint);
    if (!reader) ReleaseSRWLockShared(&readers_lock);
    return reader;
}

static void cancel_read(PendingRead *reader) {
    if (!reader->pending) return;
    CancelIoEx(reader->device, &reader->io);
    DWORD ignored;
    // The request owns the buffer until Windows reports completion.
    GetOverlappedResult(reader->device, &reader->io, &ignored, TRUE);
    reader->pending = FALSE;
}

static void clear_readers(usb_dev_handle *owner) {
    AcquireSRWLockExclusive(&readers_lock);
    PendingRead **entry = &readers;
    while (*entry) {
        PendingRead *reader = *entry;
        if (reader->owner != owner) {
            entry = &reader->next;
            continue;
        }
        *entry = reader->next;
        cancel_read(reader);
        CloseHandle(reader->io.hEvent);
        HeapFree(GetProcessHeap(), 0, reader);
    }
    ReleaseSRWLockExclusive(&readers_lock);
    if (read_state.handle == owner) read_state.handle = NULL;
}

static DWORD receive_chunk(PendingRead *reader, DWORD size, int timeout) {
    DWORD transferred = 0;
    if (!reader->pending) {
        libusb_request request = {0};
        request.endpoint.endpoint = (unsigned int)reader->endpoint;
        HANDLE event = reader->io.hEvent;
        reader->io = (OVERLAPPED){0};
        reader->io.hEvent = event;
        if (!ResetEvent(event)) return GetLastError();
        if (DeviceIoControl(reader->device, LIBUSB_IOCTL_INTERRUPT_OR_BULK_READ,
                &request, sizeof(request), reader->bytes, size, &transferred, &reader->io)) {
            reader->count = transferred;
            return ERROR_SUCCESS;
        }
        const DWORD error = GetLastError();
        if (error != ERROR_IO_PENDING) return error;
        reader->pending = TRUE;
    }

    const DWORD wait = WaitForSingleObject(reader->io.hEvent, timeout ? (DWORD)timeout : INFINITE);
    if (wait == WAIT_TIMEOUT) return ERROR_SEM_TIMEOUT;
    if (wait != WAIT_OBJECT_0) {
        const DWORD error = GetLastError();
        cancel_read(reader);
        return error;
    }

    if (!GetOverlappedResult(reader->device, &reader->io, &transferred, FALSE)) {
        const DWORD error = GetLastError();
        if (error == ERROR_IO_INCOMPLETE) return ERROR_SEM_TIMEOUT;
        reader->pending = FALSE;
        return error;
    }
    reader->pending = FALSE;
    reader->count = transferred;
    return ERROR_SUCCESS;
}

static int read_chunk(usb_dev_handle *owner, int ep, char *bytes, DWORD size, int timeout) {
    PendingRead *reader = lock_reader(owner, ep);
    if (!reader) {
        SetLastError(ERROR_NOT_ENOUGH_MEMORY);
        return USB_IO_ERROR;
    }
    AcquireSRWLockExclusive(&reader->lock);
    const DWORD error = reader->offset < reader->count ? ERROR_SUCCESS :
        receive_chunk(reader, size, timeout);
    DWORD copied = 0;
    if (error == ERROR_SUCCESS) {
        const DWORD remaining = reader->count - reader->offset;
        copied = size < remaining ? size : remaining;
        CopyMemory(bytes, reader->bytes + reader->offset, copied);
        reader->offset += copied;
        if (reader->offset == reader->count) reader->offset = reader->count = 0;
    }
    ReleaseSRWLockExclusive(&reader->lock);
    ReleaseSRWLockShared(&readers_lock);
    SetLastError(error);
    if (error == ERROR_SUCCESS) return (int)copied;
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
    const int result = read_chunk(handle, ep, bytes, chunk, timeout);
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
    clear_readers(handle);
    return original_close(handle);
}

int __cdecl mirroleap_usb_release_interface(usb_dev_handle *handle, int interface_number) {
    if (!InitOnceExecuteOnce(&resolve_once, resolve_original, NULL, NULL)) return USB_IO_ERROR;
    clear_readers(handle);
    return original_release(handle, interface_number);
}

int __cdecl mirroleap_usb_control_msg(usb_dev_handle *handle, int request_type, int request,
        int value, int index, char *bytes, int size, int timeout) {
    if (!InitOnceExecuteOnce(&resolve_once, resolve_original, NULL, NULL)) return USB_IO_ERROR;
    // QuickTime handshake restart or USB mode switch invalidates old stream bytes.
    if (request_type == 0x40 && (request == 0x40 || request == 0x52)) clear_readers(handle);
    return original_control(handle, request_type, request, value, index, bytes, size, timeout);
}
