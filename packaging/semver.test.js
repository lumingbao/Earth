const assert = require("assert").strict;
const fs = require("fs");
const path = require("path");
const vm = require("vm");

function test(name, run) {
    run();
    console.log(`PASS: ${name}`);
}

const source = fs.readFileSync(path.join(__dirname, "semver.js"), "utf8");
const comparator = vm.createContext({});
vm.runInContext(source, comparator);
const compare = comparator.earthCompareVersions;

test("SemVer ordering includes numeric prereleases and stable transitions", () => {
    const ordered = [
        "0.5.0-alpha",
        "0.5.0-alpha.1",
        "0.5.0-alpha.beta",
        "0.5.0-beta",
        "0.5.0-beta.2",
        "0.5.0-beta.9",
        "0.5.0-beta.10",
        "0.5.0-beta.9007199254740992",
        "0.5.0-beta.9007199254740993",
        "0.5.0-beta.alpha",
        "0.5.0-rc.1",
        "0.5.0",
        "0.5.1-beta.1",
        "1.0.0",
    ];
    for (let i = 0; i < ordered.length; i++) {
        for (let j = 0; j < ordered.length; j++) {
            assert.equal(compare(ordered[i], ordered[j]), Math.sign(i - j));
        }
    }
});

test("build metadata has no precedence and invalid metadata fails closed", () => {
    assert.equal(compare("1.0.0+build2", "1.0.0+build1"), 0);
    assert.equal(compare("1.0.0-beta.2+build2", "1.0.0-beta.2+build1"), 0);
    assert.equal(compare("1.0.0-A", "1.0.0-a"), -1);
    for (const invalid of [
        "", "01.0.0", "1.0", "1.0.0.0", "1.0.0-beta.01", "1.0.0-",
        "1.0.0+", "1.0.0-beta..1", "1.0.0\n", " 1.0.0", null, undefined,
    ]) {
        assert.equal(compare(invalid, "1.0.0"), null);
        assert.equal(compare("1.0.0", invalid), null);
    }
});

function windowsCheck(installed, offered, readError) {
    let exit;
    const context = vm.createContext({
        WScript: {
            Arguments: { length: 1, Item: () => offered },
            Quit: code => { exit = code; },
        },
        ActiveXObject: function (name) {
            assert.equal(name, "WScript.Shell");
            return {
                RegRead: key => {
                    assert.equal(key, "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Earth\\DisplayVersion");
                    if (readError) throw readError;
                    if (installed === undefined) {
                        const error = new Error("No registration");
                        error.number = -2147024894;
                        throw error;
                    }
                    return installed;
                },
            };
        },
    });
    vm.runInContext(source, context);
    return exit;
}

test("Windows entry blocks same-core prerelease and stable downgrades", () => {
    assert.equal(windowsCheck("0.5.0-beta.10", "0.5.0-beta.2"), 2);
    assert.equal(windowsCheck("0.5.0", "0.5.0-rc.1"), 2);
    assert.equal(windowsCheck("0.5.0-rc.1", "0.5.0-beta.10"), 2);
    assert.equal(windowsCheck("0.5.0-beta.2", "0.5.0-beta.10"), 0);
    assert.equal(windowsCheck("0.5.0-rc.1", "0.5.0"), 0);
    assert.equal(windowsCheck("0.5.0+old", "0.5.0+new"), 0);
    assert.equal(windowsCheck("invalid", "0.5.0"), 3);
    assert.equal(windowsCheck(undefined, "0.5.0"), 0);
    assert.equal(windowsCheck(undefined, "0.5.0", { number: -2147024891 }), 3);
});

function macosCheck(installed, offered, readError = false) {
    const template = fs.readFileSync(path.join(__dirname, "macos/Distribution.xml"), "utf8");
    const rendered = template.replace("__VERSION_CHECK_SCRIPT__", source).split("__VERSION__").join(offered);
    const script = rendered.match(/<script><!\[CDATA\[([\s\S]*?)\]\]><\/script>/)[1];
    const context = vm.createContext({
        system: {
            files: {
                fileExistsAtPath: value => {
                    assert.equal(value, "/Applications/Earth.app");
                    return installed !== undefined;
                },
                plistAtPath: value => {
                    assert.equal(value, "/Applications/Earth.app/Contents/Info.plist");
                    if (readError) throw new Error("Unreadable plist");
                    return installed;
                },
            },
        },
        my: { result: {} },
    });
    vm.runInContext(script, context);
    return context.earthInstallationCheck();
}

test("macOS installation-check enforces full signed version and legacy fallback", () => {
    const plist = value => ({ EarthVersion: value, CFBundleShortVersionString: "0.5.0" });
    assert.equal(macosCheck(plist("0.5.0-beta.10"), "0.5.0-beta.2"), false);
    assert.equal(macosCheck(plist("0.5.0"), "0.5.0-rc.1"), false);
    assert.equal(macosCheck(plist("0.5.0-beta.2"), "0.5.0-beta.10"), true);
    assert.equal(macosCheck(plist("0.5.0-rc.1"), "0.5.0"), true);
    assert.equal(macosCheck(plist("0.5.0+old"), "0.5.0+new"), true);
    assert.equal(macosCheck({ CFBundleShortVersionString: "0.4.0" }, "0.5.0"), true);
    assert.equal(macosCheck(plist(""), "0.5.0"), false);
    assert.equal(macosCheck({}, "0.5.0"), false);
    assert.equal(macosCheck({}, "0.5.0", true), false);
    assert.equal(macosCheck(undefined, "0.5.0"), true);
});
