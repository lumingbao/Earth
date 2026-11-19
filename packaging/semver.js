// Shared by macOS Installer JavaScript and Windows Script Host (JScript).
// Keep ES3 syntax: both native installer engines predate modern JavaScript.
function earthParseVersion(value) {
    if (typeof value !== "string" || value.length > 512) {
        return null;
    }
    var match = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(value);
    if (!match || match[0] !== value) {
        return null;
    }
    var prerelease = match[4] ? match[4].split(".") : [];
    for (var i = 0; i < prerelease.length; i++) {
        if (/^[0-9]+$/.test(prerelease[i]) &&
            prerelease[i].length > 1 && prerelease[i].charAt(0) === "0") {
            return null;
        }
    }
    return { core: [match[1], match[2], match[3]], prerelease: prerelease };
}

function earthCompareNumeric(left, right) {
    // Compare decimal strings without floating-point rounding or integer overflow.
    if (left.length !== right.length) {
        return left.length < right.length ? -1 : 1;
    }
    return left === right ? 0 : (left < right ? -1 : 1);
}

// Returns installed-versus-offered precedence, or null for invalid metadata.
function earthCompareVersions(installed, offered) {
    var left = earthParseVersion(installed);
    var right = earthParseVersion(offered);
    if (!left || !right) {
        return null;
    }
    var i, comparison;
    for (i = 0; i < 3; i++) {
        comparison = earthCompareNumeric(left.core[i], right.core[i]);
        if (comparison !== 0) {
            return comparison;
        }
    }
    if (left.prerelease.length === 0 || right.prerelease.length === 0) {
        if (left.prerelease.length === right.prerelease.length) {
            return 0;
        }
        return left.prerelease.length === 0 ? 1 : -1;
    }
    for (i = 0; i < left.prerelease.length && i < right.prerelease.length; i++) {
        var a = left.prerelease[i];
        var b = right.prerelease[i];
        if (a === b) {
            continue;
        }
        var aNumeric = /^[0-9]+$/.test(a);
        var bNumeric = /^[0-9]+$/.test(b);
        if (aNumeric && bNumeric) {
            return earthCompareNumeric(a, b);
        }
        if (aNumeric !== bNumeric) {
            return aNumeric ? -1 : 1;
        }
        return a < b ? -1 : 1;
    }
    return left.prerelease.length === right.prerelease.length ? 0 :
        (left.prerelease.length < right.prerelease.length ? -1 : 1);
}

function earthWindowsInstallCheck() {
    if (WScript.Arguments.length !== 1) {
        return 3;
    }
    var installed;
    var shell = new ActiveXObject("WScript.Shell");
    try {
        installed = shell.RegRead("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Earth\\DisplayVersion");
    } catch (error) {
        // NSIS separately refuses an existing executable without readable,
        // matching installation registration before any files are replaced.
        return error.number === -2147024894 || error.number === -2147024893 ? 0 : 3;
    }
    var comparison = earthCompareVersions(installed, WScript.Arguments.Item(0));
    return comparison === null ? 3 : (comparison > 0 ? 2 : 0);
}

if (typeof WScript !== "undefined") {
    WScript.Quit(earthWindowsInstallCheck());
}
