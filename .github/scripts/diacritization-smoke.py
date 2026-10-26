#!/usr/bin/env python3
"""Check both real models through the running image's public HTTP interface."""

import argparse
import http.cookiejar
import json
import re
import time
import urllib.error
import urllib.parse
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="http://127.0.0.1:3000")
    args = parser.parse_args()
    base = args.url.rstrip("/")
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()),
    )

    def request(path, form=None):
        data = None if form is None else urllib.parse.urlencode(form).encode()
        with opener.open(base + path, data=data, timeout=10) as response:
            return response.read(128 * 1024).decode("utf-8")

    deadline = time.monotonic() + 120
    while 'id="diacritize-form"' not in request("/diacritize"):
        if time.monotonic() >= deadline:
            raise RuntimeError("The image's diacritization models did not initialize")
        time.sleep(0.5)

    source = "اللغة العربية جميلة، وهذا اختبار للتشكيل."
    for model in ("shakkelha", "catt"):
        reservation = json.loads(request("/diacritize/jobs", {}))
        job = json.loads(request(reservation["text"], {"model": model, "text": source}))
        deadline = time.monotonic() + 60
        while True:
            try:
                output = request(job["next"] + "/result")
                break
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    raise
                error.close()
                if time.monotonic() >= deadline:
                    raise RuntimeError(f"The image's {model} job did not finish") from error
                time.sleep(0.2)
        if re.sub("[\u064b-\u0652]", "", output) != source:
            raise RuntimeError(f"{model} changed the source text")
        if not re.search("[\u064b-\u0652]", output):
            raise RuntimeError(f"{model} returned no diacritics")
        print(f"{model}: real CPU inference and text download passed")


if __name__ == "__main__":
    main()
