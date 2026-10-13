/* Web platform layer for wasi-libc (no emscripten). See docs/adr/0007-web-without-emscripten.md.
 *
 * wasi-libc routes every syscall through __wasi_* wrapper functions. Defining them here keeps
 * libc working (printf, strtod, qsort, ...) while the module imports nothing from WASI, only
 * the host functions below, which our JS provides (tools/wasm_run.mjs for tests).
 *
 * Some wrappers live together in one libc object; if a link fails with "duplicate symbol
 * __wasi_*", some libc function now references a wrapper missing here. Add it. */
#include <wasi/api.h>

#include <errno.h>
#include <stdio.h>
#include <string.h>

/* ---- Host interface (module "env") ---------------------------------------------------------- */

__attribute__((import_module("env"), import_name("titan_write"))) extern void
titan_host_write(int32_t fd, const uint8_t *ptr, size_t len);

__attribute__((import_module("env"), import_name("titan_now"))) extern double
titan_host_now(void); /* milliseconds, monotonic */

__attribute__((import_module("env"), import_name("titan_exit"))) _Noreturn extern void titan_host_exit(int32_t code);

/* ---- stdout/stderr and time ----------------------------------------------------------------- */

__wasi_errno_t __wasi_fd_write(__wasi_fd_t fd, const __wasi_ciovec_t *iovs, size_t iovs_len, __wasi_size_t *written) {
    if (fd != 1 && fd != 2) {
        return __WASI_ERRNO_BADF;
    }
    size_t total = 0;
    for (size_t i = 0; i < iovs_len; i++) {
        titan_host_write((int32_t)fd, iovs[i].buf, iovs[i].buf_len);
        total += iovs[i].buf_len;
    }
    *written = (__wasi_size_t)total;
    return __WASI_ERRNO_SUCCESS;
}

__wasi_errno_t __wasi_fd_fdstat_get(__wasi_fd_t fd, __wasi_fdstat_t *stat) {
    if (fd > 2) {
        return __WASI_ERRNO_BADF;
    }
    memset(stat, 0, sizeof *stat);
    stat->fs_filetype = __WASI_FILETYPE_CHARACTER_DEVICE;
    stat->fs_rights_base = __WASI_RIGHTS_FD_WRITE;
    return __WASI_ERRNO_SUCCESS;
}

__wasi_errno_t __wasi_clock_time_get(__wasi_clockid_t id, __wasi_timestamp_t precision, __wasi_timestamp_t *time) {
    (void)id;
    (void)precision;
    *time = (__wasi_timestamp_t)(titan_host_now() * 1e6);
    return __WASI_ERRNO_SUCCESS;
}

_Noreturn void __wasi_proc_exit(__wasi_exitcode_t code) {
    titan_host_exit((int32_t)code);
}

/* ---- Unsupported: there are no file descriptors besides stdout/stderr ----------------------- */

__wasi_errno_t __wasi_fd_close(__wasi_fd_t fd) {
    (void)fd;
    return __WASI_ERRNO_SUCCESS;
}

__wasi_errno_t __wasi_fd_seek(__wasi_fd_t fd, __wasi_filedelta_t offset, __wasi_whence_t whence,
                              __wasi_filesize_t *result) {
    (void)fd;
    (void)offset;
    (void)whence;
    (void)result;
    return __WASI_ERRNO_SPIPE;
}

__wasi_errno_t __wasi_fd_renumber(__wasi_fd_t from, __wasi_fd_t to) {
    (void)from;
    (void)to;
    return __WASI_ERRNO_BADF;
}

/* No filesystem on the web: assets go through the engine, not libc. Defining fopen keeps
 * wasi-libc's preopen/path machinery (and ~30 path_* syscalls) out of the link. */
FILE *fopen(const char *restrict path, const char *restrict mode) {
    (void)path;
    (void)mode;
    errno = ENOENT;
    return nullptr;
}
