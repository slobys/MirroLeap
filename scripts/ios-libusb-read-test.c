// SPDX-License-Identifier: MIT
// Exercise reading across polling deadlines and selective cancellation on close.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <assert.h>

static DWORD simulated_error;
static DWORD requested_size;
static unsigned submitted_reads;
static unsigned canceled_reads;
static BOOL WINAPI usb_test_read(HANDLE handle, DWORD code, LPVOID request, DWORD input_size,
        LPVOID buffer, DWORD size, LPDWORD transferred, LPOVERLAPPED io) {
    (void)code;
    (void)request;
    (void)input_size;
    requested_size = size;
    ++submitted_reads;
    if (simulated_error) {
        SetLastError(simulated_error);
        return FALSE;
    }
    return ReadFile(handle, buffer, size, transferred, io);
}
static BOOL WINAPI usb_test_cancel(HANDLE handle, LPOVERLAPPED io) {
    assert(io);
    ++canceled_reads;
    return CancelIoEx(handle, io);
}
static DWORD WINAPI usb_test_temp_path(DWORD size, LPWSTR buffer) {
    (void)size;
    (void)buffer;
    return 0;
}

#define DeviceIoControl usb_test_read
#define CancelIoEx usb_test_cancel
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

    char output[LIBUSB_MAX_READ_WRITE + 1] = {0};
    assert(mirroleap_bulk_read(&dev, 0x81, output, sizeof(output), 10) == USB_TRANSFER_TIMED_OUT);
    assert(GetLastError() == ERROR_SEM_TIMEOUT && requested_size == LIBUSB_MAX_READ_WRITE);
    assert(mirroleap_bulk_read(&dev, 0x81, output, sizeof(output), 10) == USB_TRANSFER_TIMED_OUT);
    assert(submitted_reads == 1 && canceled_reads == 0);
    assert(!GetOverlappedResult(reader, &other_read, &transferred, FALSE));
    assert(GetLastError() == ERROR_IO_INCOMPLETE);
    assert(WriteFile(writer, "A", 1, &transferred, NULL));
    assert(GetOverlappedResult(reader, &other_read, &transferred, TRUE));
    assert(transferred == 1 && other_byte == 'A');

    assert(WriteFile(writer, "BC", 2, &transferred, NULL));
    assert(output[0] == 0);
    assert(mirroleap_bulk_read(&dev, 0x81, output, 1, 100) == 1);
    assert(output[0] == 'B');
    assert(mirroleap_bulk_read(&dev, 0x81, output, 1, 100) == 1);
    assert(output[0] == 'C');
    assert(submitted_reads == 1 && canceled_reads == 0);

    ResetEvent(other_read.hEvent);
    assert(!ReadFile(reader, &other_byte, 1, &transferred, &other_read));
    assert(GetLastError() == ERROR_IO_PENDING);
    assert(mirroleap_bulk_read(&dev, 0x81, output, sizeof(output), 10) == USB_TRANSFER_TIMED_OUT);
    clear_readers(&dev);
    assert(submitted_reads == 2 && canceled_reads == 1);
    assert(!find_reader(&dev, 0x81));
    assert(!GetOverlappedResult(reader, &other_read, &transferred, FALSE));
    assert(GetLastError() == ERROR_IO_INCOMPLETE);
    assert(WriteFile(writer, "E", 1, &transferred, NULL));
    assert(GetOverlappedResult(reader, &other_read, &transferred, TRUE));
    assert(transferred == 1 && other_byte == 'E');

    assert(WriteFile(writer, "D", 1, &transferred, NULL));
    assert(mirroleap_bulk_read(&dev, 0x81, output, 1, 100) == 1 && output[0] == 'D');
    simulated_error = ERROR_GEN_FAILURE;
    assert(mirroleap_bulk_read(&dev, 0x81, output, 2, 100) == USB_TRANSFER_TIMED_OUT);
    simulated_error = ERROR_DEVICE_NOT_CONNECTED;
    assert(mirroleap_bulk_read(&dev, 0x81, output, 2, 100) == USB_IO_ERROR);
    assert(GetLastError() == ERROR_DEVICE_NOT_CONNECTED);

    clear_readers(&dev);
    assert(canceled_reads == 1);
    CloseHandle(other_read.hEvent);
    CloseHandle(writer);
    CloseHandle(reader);
    puts("USB read tests passed: persistent reads, retained bytes, targeted cleanup and device removal");
    return 0;
}
