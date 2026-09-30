#ifndef C_HEADER_H
#define C_HEADER_H

typedef struct Handle {
    int fd;
} Handle;

int open_handle(const char *path);

static inline int is_open(const Handle *handle) {
    return handle->fd >= 0;
}

#endif
