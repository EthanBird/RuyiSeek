# Source this file before building to make the musl cross-toolchain
# discoverable. The wrapper at /home/syc/musl-stage/x86_64-linux-musl-gcc
# invokes the system gcc with -specs/-B/-isystem/-L flags that point at
# the staged musl sysroot, so vendored C code (libdbus) compiles and
# links against musl's libc.a instead of glibc.
#
#   . /home/syc/RuyiSeek/.cargo/env.sh
#   cargo build --release --workspace
#
export PATH="/home/syc/musl-stage:/home/syc/.cargo/bin:$PATH"