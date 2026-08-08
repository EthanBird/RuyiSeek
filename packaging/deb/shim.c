/* shim.c — workaround for the glibc→musl toolchain mismatch.
 *
 * When building for x86_64-unknown-linux-musl without a real
 * musl-gcc, the vendored libdbus (and any other C object) is
 * compiled by the system gcc, and the system ld still searches
 * /usr/lib/x86_64-linux-gnu by default. That glibc tree ships a
 * libdl.a whose dlopen.o / dlclose.o / dlerror.o are stubs that
 * reference the private symbols __dlopen, __dlclose and
 * __dlerror — those only exist inside glibc itself, never in musl.
 *
 * musl's libc.a already provides real dlopen / dlsym / dlerror
 * directly, so we never actually need the system libdl.a at all.
 * We satisfy ld's undefined references to __dl{open,close,error}
 * with empty weak stubs; if the real __dl* symbols are later
 * linked in (they never are, with musl) the weak symbols are
 * overridden.
 */

__attribute__((weak)) void *__dlopen(const char *a, int b) {
    (void)a; (void)b;
    return (void *)0;
}

__attribute__((weak)) int __dlclose(void *a) {
    (void)a;
    return 0;
}

__attribute__((weak)) int __dlerror(void *a, int b, void *c) {
    (void)a; (void)b; (void)c;
    return 0;
}
