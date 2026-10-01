"""Record macOS app memory, including its WebKit resource coalition and compressed pages.

Coalition ABI: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info_private.h
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    proc = ctypes.CDLL("/usr/lib/libproc.dylib")
    proc.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
    proc.proc_pidinfo.restype = ctypes.c_int

    def coalition(pid):
        buffer = (ctypes.c_uint64 * 5)()
        size = proc.proc_pidinfo(pid, 20, 0, buffer, ctypes.sizeof(buffer))
        return buffer[0] if size == ctypes.sizeof(buffer) else None

    app_coalition = coalition(args.pid)
    assert app_coalition, "Cannot identify the app resource coalition"
    assert app_coalition != coalition(os.getpid()), (
        "The app shares the measuring process coalition; launch its bundle through macOS "
        "Launch Services before measuring, otherwise unrelated parent processes are counted"
    )
    members = []
    for line in subprocess.check_output(["ps", "-axo", "pid=,rss=,%cpu=,comm="], text=True).splitlines():
        fields = line.strip().split(None, 3)
        if len(fields) != 4:
            continue
        pid = int(fields[0])
        if coalition(pid) == app_coalition:
            members.append({"pid": pid, "rss_kib": int(fields[1]), "cpu_percent": float(fields[2]),
                            "name": Path(fields[3]).name})
    with tempfile.TemporaryDirectory(prefix="amberize-footprint-") as directory:
        path = Path(directory) / "footprint.json"
        subprocess.run(["footprint", *[str(member["pid"]) for member in members], "--noCategories", "--swapped", "-j", str(path)],
                       check=True, stdout=subprocess.DEVNULL)
        footprint = json.loads(path.read_text())
    assert not footprint.get("errors"), footprint.get("errors")
    for member in members:
        measured = next((item for item in footprint["processes"] if item["pid"] == member["pid"]), None)
        assert measured, f"Process {member['pid']} was not measured"
        member["physical_footprint_bytes"] = measured["auxiliary"]["phys_footprint"]
        member["translated"] = measured.get("translated", False)
    result = {"label": args.label, "time_unix": time.time(), "app_pid": args.pid,
              "resource_coalition": app_coalition, "processes": members,
              "physical_footprint_bytes": sum(member["physical_footprint_bytes"] for member in members),
              "rss_bytes": sum(member["rss_kib"] * 1024 for member in members),
              "cpu_percent_snapshot": sum(member["cpu_percent"] for member in members)}
    results = json.loads(args.report.read_text()) if args.report.exists() else []
    results.append(result)
    args.report.write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
