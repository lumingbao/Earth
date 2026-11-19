#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$ROOT_DIR"

VERSION="$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "earth"))')"
[[ -n "$VERSION" ]] || { echo "Could not read Earth version" >&2; exit 1; }
CORE_VERSION="${VERSION%%[-+]*}"
CHANNEL="${EARTH_RELEASE_CHANNEL:-development}"
if [[ "$CHANNEL" != "official" && "$CHANNEL" != "development" ]]; then
    echo "EARTH_RELEASE_CHANNEL must be official or development" >&2; exit 1
fi
if [[ "$CHANNEL" == "official" ]]; then
    : "${EARTH_UPDATE_MACOS_TEAM_ID:?Official releases require a pinned Team ID}"
    : "${EARTH_MACOS_SIGN_IDENTITY:?Developer ID Application identity required}"
    : "${EARTH_MACOS_INSTALLER_IDENTITY:?Developer ID Installer identity required}"
    : "${EARTH_NOTARY_PROFILE:?A notarytool keychain profile is required}"
    [[ "$EARTH_UPDATE_MACOS_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] || {
        echo "Official releases require a valid 10-character Team ID" >&2; exit 1
    }
fi

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

TARGET_DIR="$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
BINARIES=()
for tgt in "${BUILD_TARGETS[@]}"; do
    echo "==> 编译 Release 目标: ${tgt}"
    cargo build --locked --release --target "${tgt}"
    BINARIES+=("${TARGET_DIR}/${tgt}/release/earth")
done

DIST_DIR="${ROOT_DIR}/dist"
APP_BUNDLE="${DIST_DIR}/Earth.app"
DMG_PATH="${DIST_DIR}/Earth-${VERSION}-macOS-${ARCH_LABEL}.dmg"
PKG_PATH="${DIST_DIR}/Earth-${VERSION}-macOS-${ARCH_LABEL}.pkg"

if [[ "$CHANNEL" == "development" ]]; then
    echo "WARNING: development artifacts are ad-hoc signed, unnotarized, and not eligible for automatic updates." >&2
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

# 1. 生成 macOS .icns 图标
echo "==> 生成 Earth.icns 应用图标"
ICON_PNG="${TMP_DIR}/earth_1024.png"
ICONSET_DIR="${TMP_DIR}/Earth.iconset"
ICNS_PATH="${TMP_DIR}/Earth.icns"
mkdir -p "${ICONSET_DIR}"

swift -module-cache-path "${TMP_DIR}/swift-module-cache" \
    "${ROOT_DIR}/packaging/macos/gen_icon.swift" "${ICON_PNG}"

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
sed -e "s/__VERSION__/${VERSION}/g" -e "s/__CORE_VERSION__/${CORE_VERSION}/g" \
    "${ROOT_DIR}/packaging/macos/Info.plist" > "${APP_BUNDLE}/Contents/Info.plist"
printf 'APPL????' > "${APP_BUNDLE}/Contents/PkgInfo"

if [[ "$CHANNEL" == "official" ]]; then
    codesign --force --options runtime --timestamp --sign "$EARTH_MACOS_SIGN_IDENTITY" "$APP_BUNDLE"
    REQUIREMENT="anchor apple generic and identifier \"io.earth.Earth\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"${EARTH_UPDATE_MACOS_TEAM_ID}\""
    codesign --verify --deep --strict --all-architectures --test-requirement "$REQUIREMENT" "$APP_BUNDLE"
    ditto -c -k --keepParent "$APP_BUNDLE" "$TMP_DIR/Earth-notarize.zip"
    xcrun notarytool submit "$TMP_DIR/Earth-notarize.zip" --keychain-profile "$EARTH_NOTARY_PROFILE" --wait
    xcrun stapler staple "$APP_BUNDLE"
    spctl --assess --type execute "$APP_BUNDLE"
else
    codesign --force --sign - "$APP_BUNDLE"
    codesign --verify --deep --strict "$APP_BUNDLE"
fi

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
    --version "${CORE_VERSION}" \
    --install-location "/Applications" \
    "${COMPONENT_PKG}" >/dev/null

if [[ "${ARCH_LABEL}" == "arm64" ]]; then
    HOST_ARCHES="arm64"
elif [[ "${ARCH_LABEL}" == "x86_64" ]]; then
    HOST_ARCHES="x86_64"
else
    HOST_ARCHES="arm64,x86_64"
fi

python3 - "$ROOT_DIR" "$VERSION" "$CORE_VERSION" "$HOST_ARCHES" "$DIST_XML" <<'PY'
from pathlib import Path
import sys
root, version, core, arches, output = sys.argv[1:]
root = Path(root)
distribution = (root / "packaging/macos/Distribution.xml").read_text()
distribution = distribution.replace("__VERSION_CHECK_SCRIPT__", (root / "packaging/semver.js").read_text())
distribution = distribution.replace("__VERSION__", version).replace("__CORE_VERSION__", core)
distribution = distribution.replace("arm64,x86_64", arches)
Path(output).write_text(distribution)
PY

productbuild \
    --distribution "${DIST_XML}" \
    --resources "${ROOT_DIR}/packaging/macos" \
    --package-path "${TMP_DIR}" \
    "${PKG_PATH}" >/dev/null

if [[ "$CHANNEL" == "official" ]]; then
    productsign --sign "$EARTH_MACOS_INSTALLER_IDENTITY" "$PKG_PATH" "$TMP_DIR/Earth-signed.pkg"
    mv "$TMP_DIR/Earth-signed.pkg" "$PKG_PATH"
    codesign --timestamp --sign "$EARTH_MACOS_SIGN_IDENTITY" "$DMG_PATH"
    for artifact in "$PKG_PATH" "$DMG_PATH"; do
        xcrun notarytool submit "$artifact" --keychain-profile "$EARTH_NOTARY_PROFILE" --wait
        xcrun stapler staple "$artifact"
        xcrun stapler validate "$artifact"
    done
    pkgutil --check-signature "$PKG_PATH"
    spctl --assess --type install "$PKG_PATH"
    spctl --assess --type open --context context:primary-signature "$DMG_PATH"
fi

echo "==> 打包完成！输出产物："
ls -lh "${DIST_DIR}"
file "${APP_BUNDLE}/Contents/MacOS/earth"
