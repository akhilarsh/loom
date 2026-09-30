#define MAKE_HANDLER(name) int name(void) { return 0; }

MAKE_HANDLER(generated);

int declared(void) {
    return 1;
}
