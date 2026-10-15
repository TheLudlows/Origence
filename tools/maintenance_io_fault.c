/* Linux-only, opt-in synthetic-fixture LD_PRELOAD shim. Not disk exhaustion.
 * Only write/writev/pwrite/pwrite64 calls through interposed libc symbols are
 * covered; direct syscalls, mmap and io_uring are NOT intercepted.
 * ROOT must contain the regular sentinel .maintenance-io-fault-ok. MATCH is a
 * substring of the root-relative fd path. AFTER counts successful matching
 * calls (not bytes). MODE is pause or enospc. MARKER is an absent absolute file
 * outside ROOT with an existing parent. Marker failure disables injection.
 * A mutex serializes matching calls so AFTER is deterministic across threads.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <unistd.h>

static pthread_once_t once = PTHREAD_ONCE_INIT;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static char root[PATH_MAX], marker[PATH_MAX], match[PATH_MAX];
static size_t root_len;
static unsigned long long after, completed;
static int configured, enabled, paused, fired;

static int under_root(const char *path) {
    return !strncmp(path, root, root_len) && path[root_len] == '/';
}

static void configure(void) {
    const char *r = getenv("OC_MAINTENANCE_FAULT_ROOT");
    const char *m = getenv("OC_MAINTENANCE_FAULT_MATCH");
    const char *mode = getenv("OC_MAINTENANCE_FAULT_MODE");
    const char *out = getenv("OC_MAINTENANCE_FAULT_MARKER");
    const char *count = getenv("OC_MAINTENANCE_FAULT_AFTER");
    char sentinel[PATH_MAX], parent[PATH_MAX], resolved[PATH_MAX], *end;
    struct stat st;
    if (!r || !m || !*m || !mode || !out || out[0] != '/' ||
        strlen(m) >= sizeof(match) || strlen(out) >= sizeof(marker) ||
        !realpath(r, root) || !strcmp(root, "/")) return;
    root_len = strlen(root);
    if (strcmp(mode, "pause") && strcmp(mode, "enospc")) return;
    if (count) {
        if (!*count || strspn(count, "0123456789") != strlen(count)) return;
        errno = 0;
        after = strtoull(count, &end, 10);
        if (errno || *end) return;
    }
    if (snprintf(sentinel, sizeof(sentinel), "%s/.maintenance-io-fault-ok", root)
        >= (int)sizeof(sentinel)) return;
    int fd = open(sentinel, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0) return;
    int valid = !fstat(fd, &st) && S_ISREG(st.st_mode);
    close(fd);
    if (!valid) return;
    strcpy(parent, out);
    char *slash = strrchr(parent, '/');
    if (!slash || !slash[1] || !strcmp(slash + 1, ".") || !strcmp(slash + 1, "..")) return;
    char basename[PATH_MAX];
    strcpy(basename, slash + 1);
    if (slash == parent) slash[1] = '\0'; else *slash = '\0';
    if (!realpath(parent, resolved)) return;
    if (snprintf(marker, sizeof(marker), "%s/%s", resolved, basename) >= (int)sizeof(marker)) return;
    if (under_root(marker) || !strcmp(marker, root) || !lstat(marker, &st)) return;
    strcpy(match, m);
    paused = !strcmp(mode, "pause");
    configured = enabled = 1;
}

/* Caller holds lock. Only the marker uses raw open/write syscalls. */
static int record_marker(const char *path, const char *operation) {
    char text[PATH_MAX + 256];
    int n = snprintf(text, sizeof(text),
        "mode=%s\nsyscall=%s\nrelative_path=%s\nsuccessful_matching_writes=%llu\npid=%ld\nerrno=%d\n",
        paused ? "pause" : "enospc", operation, path + root_len,
        completed, (long)getpid(), paused ? 0 : ENOSPC);
    if (n < 0 || n >= (int)sizeof(text)) return 0;
    int fd = syscall(SYS_openat, AT_FDCWD, marker,
                     O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (fd < 0) return 0;
    size_t offset = 0;
    while (offset < (size_t)n) {
        ssize_t wrote = syscall(SYS_write, fd, text + offset, (size_t)n - offset);
        if (wrote < 0 && errno == EINTR) continue;
        if (wrote <= 0) { syscall(SYS_close, fd); return 0; }
        offset += (size_t)wrote;
    }
    int ok = syscall(SYS_fsync, fd) == 0;
    syscall(SYS_close, fd);
    return ok;
}

static ssize_t perform(int fd, const void *data, size_t size, off_t offset, int kind) {
    if (kind == 1) return syscall(SYS_pwrite64, fd, data, size, offset);
    if (kind == 2) return syscall(SYS_writev, fd, data, (int)size);
    return syscall(SYS_write, fd, data, size);
}

static ssize_t controlled(int fd, const void *data, size_t size, off_t offset, int kind) {
    int saved_errno = errno;
    pthread_once(&once, configure);
    if (!configured) {
        errno = saved_errno;
        return perform(fd, data, size, offset, kind);
    }
    char link[64], path[PATH_MAX];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path) - 1);
    if (n < 0 || n >= (ssize_t)sizeof(path) - 1) {
        errno = saved_errno;
        return perform(fd, data, size, offset, kind);
    }
    path[n] = '\0';
    if (!under_root(path) || !strstr(path + root_len, match)) {
        errno = saved_errno;
        return perform(fd, data, size, offset, kind);
    }
    pthread_mutex_lock(&lock);
    if (enabled && completed >= after) {
        if (!fired) {
            if (record_marker(path, kind == 1 ? "pwrite64" : kind == 2 ? "writev" : "write")) fired = 1;
            else enabled = 0;
        }
        if (fired) {
            if (paused) syscall(SYS_kill, getpid(), SIGSTOP);
            /* If a controller resumes a paused child, fail safely rather than
             * accidentally finishing the intercepted native write. */
            pthread_mutex_unlock(&lock);
            errno = ENOSPC;
            return -1;
        }
    }
    errno = saved_errno;
    ssize_t result = perform(fd, data, size, offset, kind);
    int error = errno;
    if (enabled && result > 0) completed++;
    pthread_mutex_unlock(&lock);
    errno = error;
    return result;
}

ssize_t write(int fd, const void *buf, size_t count) { return controlled(fd, buf, count, 0, 0); }
ssize_t pwrite(int fd, const void *buf, size_t count, off_t offset) { return controlled(fd, buf, count, offset, 1); }
ssize_t pwrite64(int fd, const void *buf, size_t count, off64_t offset) { return controlled(fd, buf, count, offset, 1); }
ssize_t writev(int fd, const struct iovec *iov, int count) { return controlled(fd, iov, (size_t)count, 0, 2); }
