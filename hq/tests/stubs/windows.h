/* TEST-ONLY Windows declarations to compile-check optional C HQ sink on
 * Linux CI. Not shipped in any application binary. */
#ifndef MLHQ_TEST_WINDOWS_H
#define MLHQ_TEST_WINDOWS_H
#include <stdint.h>
#include <stddef.h>
typedef void *HANDLE;
typedef void *LPVOID;
typedef uint32_t DWORD;
typedef int32_t LONG;
typedef size_t SIZE_T;
typedef const wchar_t *LPCWSTR;
#define WINAPI
#define FALSE 0
#define TRUE 1
#define FILE_MAP_ALL_ACCESS 0x000f001f
#define INFINITE 0xffffffffu
typedef uint32_t BOOL;
DWORD GetEnvironmentVariableA(const char *,char *,DWORD);
HANDLE OpenFileMappingW(DWORD,int,const wchar_t *);
void *MapViewOfFile(HANDLE,DWORD,DWORD,DWORD,SIZE_T);
HANDLE CreateEventW(void *,int,int,const wchar_t *);
HANDLE CreateThread(void *,SIZE_T,DWORD (WINAPI *)(LPVOID),LPVOID,DWORD,DWORD *);
DWORD WaitForSingleObject(HANDLE,DWORD);
int CloseHandle(HANDLE);
int UnmapViewOfFile(const void *);
int SetEvent(HANDLE);
LONG InterlockedIncrement(volatile LONG *);
LONG InterlockedExchange(volatile LONG *,LONG);
LONG InterlockedCompareExchange(volatile LONG *,LONG,LONG);
void *InterlockedExchangePointer(void * volatile *,void *);
#define MemoryBarrier() ((void)0)
#endif
