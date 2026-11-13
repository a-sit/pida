#!/usr/bin/env python3
"""PIDA – iOS-Build nach TestFlight hochladen, ohne die Tester per E-Mail/Push zu benachrichtigen.

Nur Python-Standardbibliothek, `openssl` (ES256-Signatur des API-Tokens) und `xcrun altool` (Upload).

    tools/testflight-upload.py --check                                   # Zugang prüfen, Builds auflisten
    tools/testflight-upload.py dist/release-0.6.3/PIDA_0.6.3_ios_appstore.ipa [--notify]

Ablauf: Upload mit `altool`, dann wartet das Skript, bis App Store Connect den Build fertig verarbeitet hat,
schaltet „Tester automatisch benachrichtigen“ (`autoNotifyEnabled`) aus und gibt den Build den internen Gruppen
ohne automatische Verteilung („PIDA interne Tests“). Die Tester sehen ihn in TestFlight, bekommen aber keine
E-Mail. Automatische Verteilung ist dafür in der Gruppe ausgeschaltet – Apple verteilt sonst schon während der
Verarbeitung und benachrichtigt, bevor das Skript den Build überhaupt sieht (0.6.3). `--notify` lässt die
Benachrichtigung an.

Zugang: `~/.pida-release/appstore.env` (APPLE_API_KEY, APPLE_API_ISSUER, APPLE_API_KEY_PATH; App-Store-Connect-
Teamschlüssel mit Rolle App Manager). Das Skript gibt weder den Schlüssel noch das Token aus. Es lädt nur nach
TestFlight hoch; eine Einreichung zur App-Store-Prüfung gibt es hier nicht.
"""

import argparse
import base64
import json
import os
import plistlib
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile

BUNDLE_ID = "at.asit.pida.app"
API = "https://api.appstoreconnect.apple.com"
ENV_FILE = os.path.expanduser("~/.pida-release/appstore.env")
WAIT_SECONDS = 1800  # so lange auf den Build in App Store Connect warten


class AscError(Exception):
    pass


def load_env():
    try:
        with open(ENV_FILE, encoding="utf-8") as f:
            env = dict(line.strip().split("=", 1) for line in f if "=" in line and not line.startswith("#"))
    except OSError as e:
        raise AscError(f"{ENV_FILE} nicht lesbar ({type(e).__name__})") from None
    for key in ("APPLE_API_KEY", "APPLE_API_ISSUER", "APPLE_API_KEY_PATH"):
        if not env.get(key):
            raise AscError(f"{key} fehlt in {ENV_FILE}")
    return env


def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def der_to_raw(der: bytes) -> bytes:
    # ECDSA-Signatur aus openssl (DER, SEQUENCE{r, s}) → JWS-Form r||s (je 32 Byte)
    out, i = b"", 2
    for _ in range(2):
        length = der[i + 1]
        out += der[i + 2 : i + 2 + length].lstrip(b"\0").rjust(32, b"\0")
        i += 2 + length
    return out


def token(env) -> str:
    now = int(time.time())
    header = b64url(json.dumps({"alg": "ES256", "kid": env["APPLE_API_KEY"], "typ": "JWT"}).encode())
    claims = b64url(
        json.dumps({"iss": env["APPLE_API_ISSUER"], "iat": now, "exp": now + 1200, "aud": "appstoreconnect-v1"}).encode()
    )
    result = subprocess.run(
        ["openssl", "dgst", "-sha256", "-sign", env["APPLE_API_KEY_PATH"]],
        input=f"{header}.{claims}".encode(),
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise AscError("openssl konnte das API-Token nicht signieren (Schlüsseldatei prüfen)")
    return f"{header}.{claims}.{b64url(der_to_raw(result.stdout))}"


def request(env, method, path, payload=None):
    body = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(f"{API}{path}", data=body, method=method)
    req.add_header("Authorization", f"Bearer {token(env)}")
    if body is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = resp.read()
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", "replace")
        try:
            detail = json.loads(detail)["errors"][0].get("detail", detail)
        except (ValueError, KeyError, IndexError):
            pass
        raise AscError(f"{method} {path.split('?')[0]}: HTTP {e.code}: {detail}") from None
    except urllib.error.URLError as e:
        raise AscError(f"{method} {path.split('?')[0]}: {e.reason}") from None
    return json.loads(data) if data else {}


def app_id(env) -> str:
    q = urllib.parse.urlencode({"filter[bundleId]": BUNDLE_ID, "fields[apps]": "bundleId"})
    apps = request(env, "GET", f"/v1/apps?{q}").get("data", [])
    if not apps:
        raise AscError(f"keine App {BUNDLE_ID} in App Store Connect")
    return apps[0]["id"]


def builds(env, app, version=None):
    params = {
        "filter[app]": app,
        "include": "buildBetaDetail",
        "fields[builds]": "version,processingState,uploadedDate,buildBetaDetail",
        "fields[buildBetaDetails]": "autoNotifyEnabled,internalBuildState",
        "sort": "-uploadedDate",
        "limit": "20",
    }
    if version:
        params["filter[version]"] = version
    d = request(env, "GET", "/v1/builds?" + urllib.parse.urlencode(params))
    details = {i["id"]: i["attributes"] for i in d.get("included", []) if i["type"] == "buildBetaDetails"}
    out = []
    for b in d.get("data", []):
        rel = (b.get("relationships", {}).get("buildBetaDetail") or {}).get("data") or {}
        out.append((b, rel.get("id"), details.get(rel.get("id"), {})))
    return out


def ipa_version(path) -> str:
    # CFBundleVersion aus Payload/*.app/Info.plist – so heißt der Build in App Store Connect
    with zipfile.ZipFile(path) as z:
        name = next(n for n in z.namelist() if n.count("/") == 2 and n.endswith(".app/Info.plist"))
        return str(plistlib.loads(z.read(name))["CFBundleVersion"])


def upload(env, ipa):
    cmd = ["xcrun", "altool", "--upload-app", "-f", ipa, "-t", "ios",
           "--apiKey", env["APPLE_API_KEY"], "--apiIssuer", env["APPLE_API_ISSUER"]]
    run_env = dict(os.environ, API_PRIVATE_KEYS_DIR=os.path.dirname(env["APPLE_API_KEY_PATH"]))
    result = subprocess.run(cmd, env=run_env, capture_output=True, text=True, check=False)
    lines = [l for l in (result.stdout + result.stderr).splitlines() if "UPLOAD" in l or "ERROR" in l or "error" in l]
    print("\n".join(lines[-5:]))
    if result.returncode != 0 or not any("UPLOAD SUCCEEDED" in l for l in lines):
        raise AscError("Upload mit altool fehlgeschlagen")


def wait_valid(env, app, version):
    # erst nach der Verarbeitung lässt sich der Build einer Gruppe geben
    deadline = time.time() + WAIT_SECONDS
    while time.time() < deadline:
        found = builds(env, app, version)
        if found:
            build, detail_id, detail = found[0]
            state = build["attributes"]["processingState"]
            if state == "VALID":
                return build, detail_id, detail
            if state in ("FAILED", "INVALID"):
                raise AscError(f"Build {version}: Verarbeitung bei Apple gescheitert ({state})")
        time.sleep(20)
    raise AscError(f"Build {version} ist nach {WAIT_SECONDS // 60} Minuten nicht fertig verarbeitet")


def internal_groups(env, app):
    d = request(env, "GET", f"/v1/apps/{app}/betaGroups?" + urllib.parse.urlencode(
        {"fields[betaGroups]": "name,isInternalGroup,hasAccessToAllBuilds", "limit": "50"}))
    return [g for g in d.get("data", []) if g["attributes"].get("isInternalGroup")]


def distribute(env, app, version, notify):
    build, detail_id, detail = wait_valid(env, app, version)
    if not notify and detail_id and detail.get("autoNotifyEnabled") is not False:
        request(env, "PATCH", f"/v1/buildBetaDetails/{detail_id}", {
            "data": {"type": "buildBetaDetails", "id": detail_id, "attributes": {"autoNotifyEnabled": False}}
        })
        print(f"Build {version}: „Tester automatisch benachrichtigen“ aus.")
    if not notify and detail.get("internalBuildState") == "IN_BETA_TESTING":
        print("Hinweis: Apple hatte den Build schon verteilt – für ihn kam die E-Mail womöglich bereits"
              " (automatische Verteilung in der Gruppe ausschalten).")
    for g in internal_groups(env, app):
        name = g["attributes"]["name"]
        if g["attributes"].get("hasAccessToAllBuilds"):
            print(f"Gruppe „{name}“: automatische Verteilung an – Apple gibt ihr den Build selbst.")
            continue
        request(env, "POST", f"/v1/betaGroups/{g['id']}/relationships/builds",
                {"data": [{"type": "builds", "id": build["id"]}]})
        print(f"Gruppe „{name}“: Build {version} freigegeben" + ("" if notify else " (ohne E-Mail)") + ".")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("ipa", nargs="?", help="für App Store Connect exportiertes .ipa")
    ap.add_argument("--check", action="store_true", help="nur Zugang prüfen, Builds und interne Gruppen zeigen")
    ap.add_argument("--notify", action="store_true", help="Tester wie gewohnt benachrichtigen (E-Mail/Push)")
    args = ap.parse_args()
    if not args.check and not args.ipa:
        ap.error(".ipa angeben oder --check")
    env = load_env()
    app = app_id(env)
    if args.check:
        print(f"Zugang ok: {BUNDLE_ID} (App {app})")
        for b, _, detail in builds(env, app):
            a = b["attributes"]
            print(f"  {a['version']}: {a['processingState']}, {detail.get('internalBuildState')}, "
                  f"benachrichtigen={detail.get('autoNotifyEnabled')}")
        for g in internal_groups(env, app):
            auto = "an" if g["attributes"].get("hasAccessToAllBuilds") else "aus"
            print(f"  Gruppe „{g['attributes']['name']}“: automatische Verteilung {auto}")
        return
    version = ipa_version(args.ipa)
    if builds(env, app, version):
        raise AscError(f"Build {version} gibt es schon – CFBundleVersion erhöhen")
    print(f"Lade {os.path.basename(args.ipa)} (Build {version}) nach TestFlight hoch …")
    upload(env, args.ipa)
    distribute(env, app, version, args.notify)


if __name__ == "__main__":
    try:
        main()
    except AscError as e:
        print(f"testflight-upload: {e}", file=sys.stderr)
        sys.exit(1)
