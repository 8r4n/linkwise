#define _GNU_SOURCE
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <sys/mman.h>
#include <sys/random.h>
#include <time.h>

enum {
    BLOCK_BYTES = 16 * 1024 * 1024,
    RESERVE_KIB = 128 * 1024,
    TIME_LIMIT_SECONDS = 60
};

static int available_kib(uint64_t *result)
{
    FILE *meminfo = fopen("/proc/meminfo", "r");
    if (!meminfo)
        return -1;

    char line[128];
    int found = 0;
    while (fgets(line, sizeof(line), meminfo)) {
        if (sscanf(line, "MemAvailable: %" SCNu64 " kB", result) == 1) {
            found = 1;
            break;
        }
    }
    fclose(meminfo);
    return found ? 0 : -1;
}

int main(void)
{
    uint64_t written = 0;
    const char *reason = "memory reserve reached";
    int failed = 0;
    struct timespec start;

    /* Under unexpected pressure the wipe should be killed before shutdown. */
    FILE *score = fopen("/proc/self/oom_score_adj", "w");
    if (!score)
        return 1;
    int protected = fputs("1000\n", score);
    if (fclose(score) || protected == EOF || clock_gettime(CLOCK_MONOTONIC, &start))
        return 1;

    for (;;) {
        uint64_t available;
        struct timespec now;
        if (available_kib(&available) || clock_gettime(CLOCK_MONOTONIC, &now)) {
            reason = "memory or clock statistics unavailable";
            failed = 1;
            break;
        }
        if (now.tv_sec - start.tv_sec >= TIME_LIMIT_SECONDS) {
            reason = "time limit reached";
            break;
        }
        if (available < RESERVE_KIB + BLOCK_BYTES / 1024)
            break;

        unsigned char *block = mmap(NULL, BLOCK_BYTES, PROT_READ | PROT_WRITE,
                                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (block == MAP_FAILED) {
            reason = "anonymous mapping failed";
            failed = 1;
            break;
        }

        /* Keep mappings resident until exit so later passes take other pages. */
        size_t offset = 0;
        while (offset < BLOCK_BYTES) {
            ssize_t bytes = getrandom(block + offset, BLOCK_BYTES - offset, GRND_NONBLOCK);
            if (bytes < 0 && errno == EINTR)
                continue;
            if (bytes <= 0) {
                reason = "kernel random source failed";
                failed = 1;
                break;
            }
            offset += (size_t)bytes;
            written += (uint64_t)bytes;
        }
        if (failed)
            break;
    }

    printf("Linkwise shutdown: wrote %" PRIu64 " random bytes (%s)\n", written, reason);
    return failed || written == 0;
}
