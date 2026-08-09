#!/usr/bin/env bash
# 把 ruyiseek-ui 运行所需的全部共享库 + 动态链接器打包到 $STAGING/usr/lib/ruyiseek/，
# 并 patchelf 修改二进制，让它脱离系统 /lib/x86_64-linux-gnu 也能跑。
#
# 背景：客户的目标机不能联网，apt install 不能拉 Depends。ruyiseekd / ruyi
# 是 musl 静态链接没有运行时依赖；只有 ruyiseek-ui 是动态链接的（winit 通过
# dlopen 加载 libX11.so.6 等，所以 musl-static 的 dlopen 是桩函数这条路被堵
# 死了）。本脚本把 27 个 .so + ld-linux 全部内联进 .deb，再用 patchelf 把
# 二进制的 DT_RPATH 和 PT_INTERP 改成 /usr/lib/ruyiseek，apt 就不再需要联网
# 装 X11 / fontconfig / freetype / 等等。

set -euo pipefail

STAGING="${1:?usage: bundle-libs.sh <staging-tree>}"
# Patch 进去的 RPATH 和 PT_INTERP 必须是安装后的路径（/usr/lib/ruyiseek），
# 不是当前 staging 树。.deb 会被 dpkg 解到 /，二进制运行时只在 / 下找库。
INSTALL_LIBS_DIR="/usr/lib/ruyiseek"
LIBS_DIR="$STAGING/$INSTALL_LIBS_DIR"
HELPER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

DYN_BIN="$STAGING/usr/bin/ruyiseek-ui"
if [ ! -x "$DYN_BIN" ]; then
    echo "bundle-libs.sh: binary not found in staging: $DYN_BIN" >&2
    exit 1
fi

mkdir -p "$LIBS_DIR"

# 1. 解析闭包。把 Depends: 段里声明的库 + 直接 dlopen 入口（libxkbcommon-x11、
#    libxcb-xkb、libstdc++、libgcc_s）作为种子喂给 recursive_ldd.sh。它会顺着
#    ldd 的输出把每个子库的 SONAME 加进队列，直到整张闭包稳定。
SEEDS=(
    libX11.so.6
    libxcb.so.1
    libXi.so.6
    libXcursor.so.1
    libX11-xcb.so.1
    libxkbcommon.so.0
    libxkbcommon-x11.so.0
    libxcb-xkb.so.1
    libfontconfig.so.1
    libfreetype.so.6
    libstdc++.so.6
    libgcc_s.so.1
)
echo "==> 解析 ruyiseek-ui 的运行时共享库闭包"
CLOSURE_TSV="$(mktemp)"
trap 'rm -f "$CLOSURE_TSV"' EXIT
LIB_SEARCH_DIR="/lib/x86_64-linux-gnu"
if [ ! -d "$LIB_SEARCH_DIR" ]; then
    LIB_SEARCH_DIR="/usr/lib/x86_64-linux-gnu"
fi
"$HELPER_DIR/recursive_ldd.sh" "$LIB_SEARCH_DIR" "${SEEDS[@]}" > "$CLOSURE_TSV"
COUNT=$(wc -l < "$CLOSURE_TSV")
echo "    $COUNT 个共享库（含 glibc 基础栈、X11 客户端栈、字体栈）"

# 2. 拷贝。每个 SONAME 在源目录里通常同时存在一个无版本的 .so.X 软链接和一
#    个带完整版本号的实际文件。loader 通过 SONAME 找库，所以两边都要进
#    LIBS_DIR。用 cp -P 保留软链接。
echo "==> 拷贝闭包到 $LIBS_DIR"
while IFS=$'\t' read soname path; do
    [ -z "$soname" ] && continue
    src_dir=$(dirname "$path")
    cp -P "$src_dir/$soname" "$LIBS_DIR/" 2>/dev/null || true
    cp -P "$path"             "$LIBS_DIR/"
done < "$CLOSURE_TSV"

# 3. 拷贝动态链接器本身。PT_INTERP 必须指到一个真实文件。系统的 ld-linux
#    在 /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2，它本身是个指向
#    ld-2.28.so 的软链接。-P 保留软链接形态，但目标文件（ld-2.28.so）不会
#    跟着进来——必须显式再拷一次。
LD_SRC=/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2
cp -P "$LD_SRC" "$LIBS_DIR/"
# 解析软链接拿到真实文件并拷过去，避免装到目标机后变 dangling symlink
LD_REAL=$(readlink -f "$LD_SRC")
if [ -n "$LD_REAL" ] && [ "$LD_REAL" != "$LD_SRC" ]; then
    cp -P "$LD_REAL" "$LIBS_DIR/"
fi

# 4. patchelf 二进制。把 PT_INTERP 与 DT_RPATH 都改到 LIBS_DIR。所有 dlopen
#    （包括 winit → x11-dl 走的动态加载）都会先搜 DT_RPATH。
echo "==> patchelf：PT_INTERP / DT_RPATH → $INSTALL_LIBS_DIR"
patchelf --set-interpreter "$INSTALL_LIBS_DIR/ld-linux-x86-64.so.2" "$DYN_BIN"
patchelf --set-rpath       "$INSTALL_LIBS_DIR"                         "$DYN_BIN"

# 5. 自检：ldd 现在应该全部解析到 $LIBS_DIR 而不是 /lib/...
echo "==> 自检：ldd ruyiseek-ui"
MISSING=0
while IFS= read -r line; do
    case "$line" in
        *"=> not found"*)
            echo "    FAIL $line"
            MISSING=$((MISSING + 1))
            ;;
        *)
            echo "    ok   ${line%% =>*}"
            ;;
    esac
done < <(ldd "$DYN_BIN" 2>&1 | grep ' => ' || true)

if [ "$MISSING" -gt 0 ]; then
    echo "bundle-libs.sh: $MISSING 个库没解析到，.deb 不能离线用" >&2
    exit 1
fi

echo "==> 完成：$(du -sh "$LIBS_DIR" | awk '{print $1}') 的运行时库已内联"