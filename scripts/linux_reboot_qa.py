"""Boot and reboot a disposable Ubuntu desktop guest with the exact draft Debian package."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


GUEST_SETUP = r'''
sudo cloud-init status --wait
sudo env DEBIAN_FRONTEND=noninteractive apt-get update -qq
printf 'lightdm shared/default-x-display-manager select lightdm\n' | sudo debconf-set-selections
sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends lightdm xfce4 xserver-xorg-video-dummy dbus-user-session python3
sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y /home/qa/candidate.deb
sudo passwd -d qa >/dev/null
sudo mkdir -p /etc/lightdm/lightdm.conf.d
sudo tee /etc/lightdm/lightdm.conf.d/50-amberize-qa.conf >/dev/null <<'EOF'
[Seat:*]
autologin-user=qa
autologin-user-timeout=0
user-session=xfce
EOF
sudo tee /etc/X11/xorg.conf >/dev/null <<'EOF'
Section "Device"
 Identifier "Dummy"
 Driver "dummy"
 VideoRam 256000
EndSection
Section "Monitor"
 Identifier "Monitor"
 HorizSync 28-80
 VertRefresh 48-75
 Modeline "1280x800" 83.5 1280 1352 1480 1680 800 803 809 831 -HSync +VSync
EndSection
Section "Screen"
 Identifier "Screen"
 Device "Dummy"
 Monitor "Monitor"
 DefaultDepth 24
 SubSection "Display"
  Depth 24
  Modes "1280x800"
 EndSubSection
EndSection
EOF
mkdir -p /home/qa/amberize-qa /home/qa/.config/com.amberize.app /home/qa/.config/autostart
chmod +x /home/qa/qa_fixture
/home/qa/qa_fixture create /home/qa/amberize-qa/archive.sqlite3 1024 > /home/qa/amberize-qa/before.json
python3 - <<'PY'
import json,pathlib,shutil
root=pathlib.Path('/home/qa')
binary=shutil.which('Amberize') or shutil.which('amberize')
assert binary
config={'db_path':str(root/'amberize-qa/archive.sqlite3'),'sync_interval_secs':3600}
(root/'.config/com.amberize.app/config.json').write_text(json.dumps(config))
# Match the plugin's XDG entry; the real desktop login manager must start it.
(root/'.config/autostart/Amberize.desktop').write_text('[Desktop Entry]\nType=Application\nName=Amberize\nExec='+binary+' --background\nTerminal=false\n')
PY
sudo systemctl set-default graphical.target
sudo systemctl enable lightdm
sudo systemctl restart lightdm
'''

GUEST_CHECKPOINT = r'''
python3 - <<'PY'
from contextlib import closing
import hashlib,json,pathlib,subprocess
root=pathlib.Path('/home/qa')
archive=root/'amberize-qa/archive.sqlite3'
with closing(__import__('sqlite3').connect('file:'+str(archive)+'?mode=ro',uri=True)) as connection:
 hashes=[]
 for expected,raw in connection.execute('SELECT sha256,raw_mime FROM message_blobs ORDER BY sha256'):
  actual=hashlib.sha256(raw).hexdigest()
  assert expected==actual
  hashes.append(actual)
 starts=connection.execute("SELECT COUNT(*) FROM events WHERE kind='app_started'").fetchone()[0]
 checks=connection.execute("SELECT COUNT(*) FROM events WHERE kind='integrity_check'").fetchone()[0]
sessions=[]
for line in subprocess.check_output(['loginctl','list-sessions','--no-legend','--no-pager'],text=True).splitlines():
 parts=line.split()
 if len(parts)>2 and parts[2]=='qa':
  values=subprocess.check_output(['loginctl','show-session',parts[0],'-p','Type','-p','State'],text=True)
  data=dict(row.split('=',1) for row in values.splitlines() if '=' in row)
  if data.get('Type')=='x11' and data.get('State')=='active':sessions.append(parts[0])
pids=subprocess.run(['pgrep','-x','Amberize'],capture_output=True,text=True).stdout.split()
commands=[pathlib.Path('/proc/'+pid+'/cmdline').read_bytes().split(b'\0') for pid in pids]
assert len(pids)==1 and b'--background' in commands[0], 'Background app not running'
assert sessions and starts and checks, 'Desktop login did not restore and verify the archive'
config=json.loads((root/'.config/com.amberize.app/config.json').read_text())
assert config=={'db_path':str(archive),'sync_interval_secs':3600}
assert hashes==json.loads((root/'amberize-qa/before.json').read_text())['mime_hashes']
binary=pathlib.Path(commands[0][0].decode())
print(json.dumps({'boot_id':pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
 'desktop_sessions':sessions,'app_started':starts,'integrity_checks':checks,'pid':int(pids[0]),
 'background':True,'mime_hashes_preserved':len(hashes),'saved_config_preserved':True,
 'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}))
PY
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ("root", "image", "package", "fixture-tool"):
        parser.add_argument("--" + flag, type=Path, required=True)
    parser.add_argument("--package-sha256", required=True)
    args = parser.parse_args()
    assert os.environ.get("GITHUB_ACTIONS") == "true" and os.sys.platform.startswith("linux")
    root = args.root.resolve()
    assert root.is_relative_to(Path(os.environ["RUNNER_TEMP"]).resolve()) and not root.exists()
    assert hashlib.sha256(args.package.read_bytes()).hexdigest() == args.package_sha256
    root.mkdir()
    evidence = root / "evidence"
    evidence.mkdir()
    key = root / "guest-ssh-key"
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)], check=True)
    cloud = {"users": [{"name": "qa", "shell": "/bin/bash", "groups": ["sudo"],
                        "sudo": "ALL=(ALL) NOPASSWD:ALL", "lock_passwd": True,
                        "ssh_authorized_keys": [key.with_suffix(".pub").read_text().strip()]}],
             "ssh_pwauth": False}
    (root / "user-data").write_text("#cloud-config\n" + json.dumps(cloud))
    (root / "meta-data").write_text("instance-id: amberize-disposable-reboot-qa\nlocal-hostname: amberize-qa\n")
    subprocess.run(["cloud-localds", str(root / "seed.img"), str(root / "user-data"), str(root / "meta-data")], check=True)
    disk = root / "guest.qcow2"
    shutil.copyfile(args.image, disk)
    subprocess.run(["qemu-img", "resize", str(disk), "20G"], check=True)
    firmware = Path("/usr/share/OVMF/OVMF_CODE_4M.fd")
    variables = root / "OVMF_VARS.fd"
    shutil.copyfile("/usr/share/OVMF/OVMF_VARS_4M.fd", variables)
    accelerated = os.access("/dev/kvm", os.R_OK | os.W_OK)
    command = ["qemu-system-x86_64", "-machine", "q35", "-accel", "kvm" if accelerated else "tcg",
               "-cpu", "host" if accelerated else "max", "-m", "4096", "-smp", "2",
               "-drive", f"if=pflash,format=raw,readonly=on,file={firmware}",
               "-drive", f"if=pflash,format=raw,file={variables}",
               "-drive", f"file={disk},if=virtio,format=qcow2",
               "-drive", f"file={root / 'seed.img'},if=virtio,format=raw,readonly=on",
               "-netdev", "user,id=network,hostfwd=tcp:127.0.0.1:22223-:22",
               "-device", "virtio-net-pci,netdev=network", "-display", "none",
               "-serial", f"file:{evidence / 'serial.log'}"]
    prefix = ["ssh", "-i", str(key), "-p", "22223", "-o", "BatchMode=yes",
              "-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=accept-new",
              "-o", f"UserKnownHostsFile={root / 'known-hosts'}", "qa@127.0.0.1"]
    report = {"passed": False, "actual_os_reboot": False, "platform": "Ubuntu 24.04 amd64",
              "accelerator": "kvm" if accelerated else "tcg", "package_sha256": args.package_sha256,
              "stages": [], "limitations": ["Linux only; Windows/macOS OS reboot unverified",
                                             "XDG entry seeded by test; app UI preference toggle not exercised",
                                             "Synthetic data without real mailbox credentials"]}
    vm = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=(evidence / "qemu.log").open("wb"))

    def ssh(script, timeout=120):
        return subprocess.run(prefix + ["bash", "-se"], input=script, text=True, capture_output=True, timeout=timeout)

    def checkpoint():
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline:
            assert vm.poll() is None, "Disposable VM exited"
            attempt = ssh(GUEST_CHECKPOINT)
            if attempt.returncode == 0:
                return json.loads(attempt.stdout)
            time.sleep(5)
        raise RuntimeError("Guest desktop login did not start Amberize and verify its saved archive")

    try:
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline:
            assert vm.poll() is None, "Disposable VM exited before boot"
            if ssh("true").returncode == 0:
                break
            time.sleep(5)
        else:
            raise RuntimeError("Guest SSH did not become ready")
        for source, name in ((args.package, "candidate.deb"), (args.fixture_tool, "qa_fixture")):
            scp = ["scp", "-i", str(key), "-P", "22223", "-o", "BatchMode=yes",
                   "-o", f"UserKnownHostsFile={root / 'known-hosts'}", str(source), "qa@127.0.0.1:/home/qa/" + name]
            subprocess.run(scp, check=True)
        setup = ssh(GUEST_SETUP, timeout=1200)
        (evidence / "guest-setup.log").write_text(setup.stdout + setup.stderr)
        assert setup.returncode == 0, "Guest package/desktop setup failed"
        before = checkpoint()
        report["stages"].append(before)
        ssh("sudo systemctl reboot", timeout=30)
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline:
            attempt = ssh("cat /proc/sys/kernel/random/boot_id")
            if attempt.returncode == 0 and attempt.stdout.strip() != before["boot_id"]:
                break
            time.sleep(5)
        else:
            raise RuntimeError("Guest kernel boot identity did not change")
        after = checkpoint()
        assert after["boot_id"] != before["boot_id"] and after["app_started"] > before["app_started"]
        assert after["integrity_checks"] > before["integrity_checks"]
        assert after["binary_sha256"] == before["binary_sha256"]
        report["stages"].append(after)
        verification = ssh("/home/qa/qa_fixture verify /home/qa/amberize-qa/archive.sqlite3")
        assert verification.returncode == 0
        after_archive = json.loads(verification.stdout)
        assert after_archive["integrity"]["ok"]
        (evidence / "after.json").write_text(verification.stdout)
        report["actual_os_reboot"] = True
        report["passed"] = True
    except BaseException as error:
        report["error"] = str(error)
        if vm.poll() is None:
            diagnostic = ssh("sudo journalctl -b -u lightdm --no-pager; cat /home/qa/.xsession-errors || true")
            (evidence / "desktop-diagnostics.log").write_text(diagnostic.stdout + diagnostic.stderr)
        raise
    finally:
        vm.terminate()
        try:
            vm.wait(timeout=20)
        except subprocess.TimeoutExpired:
            vm.kill()
            vm.wait(timeout=20)
        report["disposable_vm_stopped"] = vm.poll() is not None
        (evidence / "reboot-login.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
