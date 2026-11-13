#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$ROOT_DIR"

VERSION="$(python3 -c '
import re, pathlib
text = pathlib.Path("Cargo.toml").read_text()
m = re.search(r"^version\s*=\s*\"([^\"]+)\"", text, re.MULTILINE)
print(m.group(1) if m else "0.1.0")
')"

MODE="${1:-auto}" # auto | universal | native
ARCH_LABEL="universal"
BUILD_TARGETS=()

INSTALLED_TARGETS="$(rustup target list --installed)"
if [[ "$MODE" == "native" ]]; then
    HOST_ARCH="$(uname -m)"
    if [[ "$HOST_ARCH" == "arm64" ]]; then
        BUILD_TARGETS=("aarch64-apple-darwin")
        ARCH_LABEL="arm64"
    else
        BUILD_TARGETS=("x86_64-apple-darwin")
        ARCH_LABEL="x86_64"
    fi
elif [[ "$MODE" == "universal" ]] || (
    grep -q "aarch64-apple-darwin" <<<"$INSTALLED_TARGETS" &&
    grep -q "x86_64-apple-darwin" <<<"$INSTALLED_TARGETS"
); then
    BUILD_TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin")
    ARCH_LABEL="universal"
else
    HOST_ARCH="$(uname -m)"
    if [[ "$HOST_ARCH" == "arm64" ]]; then
        BUILD_TARGETS=("aarch64-apple-darwin")
        ARCH_LABEL="arm64"
    else
        BUILD_TARGETS=("x86_64-apple-darwin")
        ARCH_LABEL="x86_64"
    fi
fi

echo "==> Earth macOS 打包开始 (version=${VERSION}, arch=${ARCH_LABEL})"

BINARIES=()
for tgt in "${BUILD_TARGETS[@]}"; do
    echo "==> 编译 Release 目标: ${tgt}"
    cargo build --release --target "${tgt}"
    BINARIES+=("${ROOT_DIR}/target/${tgt}/release/earth")
done

DIST_DIR="${ROOT_DIR}/dist"
APP_BUNDLE="${DIST_DIR}/Earth.app"
DMG_PATH="${DIST_DIR}/Earth-${VERSION}-macOS-${ARCH_LABEL}.dmg"
PKG_PATH="${DIST_DIR}/Earth-${VERSION}-macOS-${ARCH_LABEL}.pkg"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

# 1. 生成 macOS .icns 图标
echo "==> 生成 Earth.icns 应用图标"
ICON_PNG="${TMP_DIR}/earth_1024.png"
ICONSET_DIR="${TMP_DIR}/Earth.iconset"
ICNS_PATH="${TMP_DIR}/Earth.icns"
mkdir -p "${ICONSET_DIR}"

swift "${ROOT_DIR}/packaging/macos/gen_icon.swift" "${ICON_PNG}"

resize_icon() {
    local px="$1"
    local out="$2"
    sips -s format png -z "${px}" "${px}" "${ICON_PNG}" --out "${ICONSET_DIR}/${out}" >/dev/null
}

resize_icon 16   "icon_16x16.png"
resize_icon 32   "icon_16x16@2x.png"
resize_icon 32   "icon_32x32.png"
resize_icon 64   "icon_32x32@2x.png"
resize_icon 128  "icon_128x128.png"
resize_icon 256  "icon_128x128@2x.png"
resize_icon 256  "icon_256x256.png"
resize_icon 512  "icon_256x256@2x.png"
resize_icon 512  "icon_512x512.png"
cp "${ICON_PNG}" "${ICONSET_DIR}/icon_512x512@2x.png"

iconutil -c icns "${ICONSET_DIR}" -o "${ICNS_PATH}"

# 2. 组装 Earth.app
echo "==> 组装 ${APP_BUNDLE}"
rm -rf "${DIST_DIR}"
mkdir -p "${APP_BUNDLE}/Contents/MacOS" "${APP_BUNDLE}/Contents/Resources"

if [[ "${#BINARIES[@]}" -gt 1 ]]; then
    lipo -create "${BINARIES[@]}" -output "${APP_BUNDLE}/Contents/MacOS/earth"
else
    cp "${BINARIES[0]}" "${APP_BUNDLE}/Contents/MacOS/earth"
fi
chmod 755 "${APP_BUNDLE}/Contents/MacOS/earth"

cp "${ICNS_PATH}" "${APP_BUNDLE}/Contents/Resources/Earth.icns"
sed "s/__VERSION__/${VERSION}/g" "${ROOT_DIR}/packaging/macos/Info.plist" > "${APP_BUNDLE}/Contents/Info.plist"
printf 'APPL????' > "${APP_BUNDLE}/Contents/PkgInfo"

SIGN_IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null | awk -F'"' '/Apple Development|Developer ID Application/ {print $2; exit}')"
if [[ -n "${SIGN_IDENTITY}" ]]; then
    echo "==> 使用开发者证书签名: ${SIGN_IDENTITY}"
    codesign --force --deep --sign "${SIGN_IDENTITY}" "${APP_BUNDLE}"
else
    echo "==> 执行 Ad-hoc 代码签名并校验"
    codesign --force --deep --sign - "${APP_BUNDLE}"
fi
codesign --verify --deep --strict "${APP_BUNDLE}"

# 3. 构建 .dmg 安装镜像
echo "==> 构建 DMG 镜像: ${DMG_PATH}"
DMG_STAGE="${TMP_DIR}/dmg_stage"
mkdir -p "${DMG_STAGE}"
ditto "${APP_BUNDLE}" "${DMG_STAGE}/Earth.app"
ln -s /Applications "${DMG_STAGE}/Applications"

hdiutil create \
    -volname "Earth ${VERSION}" \
    -srcfolder "${DMG_STAGE}" \
    -ov \
    -format UDZO \
    "${DMG_PATH}" >/dev/null
hdiutil verify "${DMG_PATH}" >/dev/null

# 4. 构建 .pkg 向导安装包
echo "==> 构建 PKG 安装程序: ${PKG_PATH}"
PKG_ROOT="${TMP_DIR}/pkg_root"
COMPONENT_PKG="${TMP_DIR}/Earth-component.pkg"
DIST_XML="${TMP_DIR}/Distribution.xml"
mkdir -p "${PKG_ROOT}"
ditto "${APP_BUNDLE}" "${PKG_ROOT}/Earth.app"

pkgbuild \
    --root "${PKG_ROOT}" \
    --component-plist "${ROOT_DIR}/packaging/macos/component.plist" \
    --identifier "io.earth.Earth.pkg" \
    --version "${VERSION}" \
    --install-location "/Applications" \
    "${COMPONENT_PKG}" >/dev/null

if [[ "${ARCH_LABEL}" == "arm64" ]]; then
    HOST_ARCHES="arm64"
elif [[ "${ARCH_LABEL}" == "x86_64" ]]; then
    HOST_ARCHES="x86_64"
else
    HOST_ARCHES="arm64,x86_64"
fi

sed -e "s/__VERSION__/${VERSION}/g" -e "s/arm64,x86_64/${HOST_ARCHES}/g" \
    "${ROOT_DIR}/packaging/macos/Distribution.xml" > "${DIST_XML}"

productbuild \
    --distribution "${DIST_XML}" \
    --resources "${ROOT_DIR}/packaging/macos" \
    --package-path "${TMP_DIR}" \
    "${PKG_PATH}" >/dev/null

echo "==> 打包完成！输出产物："
ls -lh "${DIST_DIR}"
file "${APP_BUNDLE}/Contents/MacOS/earth"
