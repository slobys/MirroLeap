#!/usr/bin/env python3
"""Narrow npm audit exception: ONE known dev-only picomatch advisory.

GHSA-fc5q-39xc-vjpf currently affects build-only Vite/Vitest dependencies.
Production high/critical advisories and every other high/critical advisory
always block CI. Remove this allowance after a patched upstream release.
"""
import json
import subprocess
import sys
ALLOWED = "https://github.com/advisories/GHSA-fc5q-39xc-vjpf"

def audit(*args):
    cmd = subprocess.run(["npm","audit","--json",*args],capture_output=True,
                         text=True,timeout=120,check=False)
    try: result=json.loads(cmd.stdout)
    except (TypeError,ValueError) as err: raise RuntimeError("npm audit returned invalid JSON") from err
    if "error" in result or "vulnerabilities" not in result:
        raise RuntimeError("npm audit returned an error or incomplete result")
    return result

def high(report):
    return {k:v for k,v in report["vulnerabilities"].items()
            if v.get("severity") in ("high","critical")}

def sources(name,report,visited=None):
    visited=set() if visited is None else visited
    if name in visited: return set()
    visited.add(name)
    finding=report["vulnerabilities"].get(name)
    if finding is None: raise RuntimeError("Unresolved advisory chain: "+name)
    result=set()
    for item in finding.get("via",[]):
        if isinstance(item,dict):
            if not item.get("url"): raise RuntimeError("Unidentified advisory on "+name)
            result.add(item["url"])
        elif isinstance(item,str):
            result |= sources(item,report,visited.copy())
        else: raise RuntimeError("Unsupported audit entry on "+name)
    if not result: raise RuntimeError("Unattributed vulnerability on "+name)
    return result

try:
    production=high(audit("--omit=dev"))
    if production: raise RuntimeError("Production high/critical: "+str(sorted(production)))
    full=audit()
    unexpected={}
    for name in high(full):
        urls=sources(name,full)
        if urls!={ALLOWED}:unexpected[name]=sorted(urls)
    if unexpected:
        raise RuntimeError("Unexpected high/critical findings: "+json.dumps(unexpected,sort_keys=True))
    print(f"WARNING: dev-only {len(high(full))} vulnerable paths from {ALLOWED}; zero production high/critical")
except (RuntimeError, subprocess.TimeoutExpired) as exc:
    print("FAIL: "+str(exc),file=sys.stderr)
    sys.exit(1)
