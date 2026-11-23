// SPDX-License-Identifier: MIT
// Exercise the real overlapped wait and cancellation on two reads on one handle.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <assert.h>

static DWORD simulated_error;
static DWORD requested_size;
static BOOL WINAPI usb_test_read(HANDLE handle, DWORD code, LPVOID request, DWORD input_size,
        LPVOID buffer, DWORD size, LPDWORD transferred, LPOVERLAPPED io) {
    (void)code;
    (void)request;
    (void)input_size;
    requested_size = size;
    if (simulated_error) {
        SetLastError(simulated_error);
        return FALSE;
    }
    return ReadFile(handle, buffer, size, transferred, io);
}
static DWORD WINAPI usb_test_temp_path(DWORD size, LPWSTR buffer) {
    (void)size;
    (void)buffer;
    return 0;
}

#define DeviceIoControl usb_test_read
#define GetTempPathW usb_test_temp_path
#include "ios-libusb-read.c"

int main(void) {
    WCHAR name[96];
    swprintf_s(name, ARRAYSIZE(name), L"\\\\.\\pipe\\MirroLeapUsbReadTest-%lu", GetCurrentProcessId());
    HANDLE reader = CreateNamedPipeW(name, PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 4096, 4096, 0, NULL);
    assert(reader != INVALID_HANDLE_VALUE);
    HANDLE writer = CreateFileW(name, GENERIC_WRITE, 0, NULL, OPEN_EXISTING, 0, NULL);
    assert(writer != INVALID_HANDLE_VALUE);
    OVERLAPPED connected = {0};
    assert(!ConnectNamedPipe(reader, &connected) && GetLastError() == ERROR_PIPE_CONNECTED);

    usb_dev_handle dev = {0};
    dev.config = 1;
    dev.impl_info = reader;
    OVERLAPPED other_read = {0};
    other_read.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
    assert(other_read.hEvent);
    char other_byte = 0;
    DWORD transferred;
    assert(!ReadFile(reader, &other_byte, 1, &transferred, &other_read));
    assert(GetLastError() == ERROR_IO_PENDING);

    char output[LIBUSB_MAX_READ_WRITE + 1];
    assert(mirroleap_bulk_read(&dev, 0x81, output, sizeof(output), 10) == USB_TRANSFER_TIMED_OUT);
    assert(GetLastError() == ERROR_SEM_TIMEOUT && requested_size == LIBUSB_MAX_READ_WRITE);
    assert(!GetOverlappedResult(reader, &other_read, &transferred, FALSE));
    assert(GetLastError() == ERROR_IO_INCOMPLETE);
    assert(WriteFile(writer, "A", 1, &transferred, NULL));
    assert(GetOverlappedResult(reader, &other_read, &transferred, TRUE));
    assert(transferred == 1 && other_byte == 'A');

    assert(WriteFile(writer, "BC", 2, &transferred, NULL));
    assert(mirroleap_bulk_read(&dev, 0x81, output, sizeof(output), 100) == 2);
    assert(output[0] == 'B' && output[1] == 'C');
    simulated_error = ERROR_GEN_FAILURE;
    assert(mirroleap_bulk_read(&dev, 0x81, output, 2, 100) == USB_TRANSFER_TIMED_OUT);
    simulated_error = ERROR_DEVICE_NOT_CONNECTED;
    assert(mirroleap_bulk_read(&dev, 0x81, output, 2, 100) == USB_IO_ERROR);
    assert(GetLastError() == ERROR_DEVICE_NOT_CONNECTED);

    CloseHandle(other_read.hEvent);
    CloseHandle(writer);
    CloseHandle(reader);
    puts("USB read tests passed: selective cancellation, 64 KiB chunks, resumed reads and device removal");
    return 0;
}
