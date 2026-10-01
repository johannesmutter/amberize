"""Exercise the exact candidate's IMAP adapter over trusted loopback TLS in fresh Linux CI."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import socket
import socketserver
import sqlite3
import ssl
import subprocess
import threading
import time


def mime(uid, generation=10):
    return (f"Message-ID: <tls-qa-{generation}-{uid}@example.invalid>\r\n"
            f"From: qa@example.invalid\r\nTo: recipient@example.invalid\r\n"
            f"Subject: Synthetic TLS QA {generation}/{uid}\r\nMIME-Version: 1.0\r\n"
            "Content-Type: multipart/mixed; boundary=qa\r\n\r\n"
            "--qa\r\nContent-Type: text/plain\r\n\r\nSynthetic protocol QA.\r\n"
            "--qa\r\nContent-Type: application/octet-stream\r\n"
            'Content-Disposition: attachment; filename="qa.txt"\r\n'
            "Content-Transfer-Encoding: base64\r\n\r\nUUEgYXR0YWNobWVudAo=\r\n--qa--\r\n").encode()


class Provider(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True
    block_on_close = False

    def __init__(self, certificate, key, port=0):
        self.tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.tls.load_cert_chain(certificate, key)
        self.messages = {uid: mime(uid) for uid in (1, 2, 3)}
        self.generation = 10
        self.mode = "normal"
        self.fail_uid = None
        self.body_requests = []
        self.requests = []
        super().__init__(("127.0.0.1", port), Handler)

    def get_request(self):
        connection, address = super().get_request()
        connection.settimeout(75)
        return self.tls.wrap_socket(connection, server_side=True), address


class Handler(socketserver.StreamRequestHandler):
    def handle(self):
        def send(value):
            self.wfile.write(value.encode() if isinstance(value, str) else value)
            self.wfile.flush()
        try:
            send("* OK [CAPABILITY IMAP4rev1] Synthetic TLS QA ready\r\n")
            while raw := self.rfile.readline(4096):
                tag, command = raw.decode().rstrip("\r\n").split(" ", 1)
                upper = command.upper()
                stage = upper.split()[0]
                # Store fixed command kinds, never LOGIN values.
                self.server.requests.append(stage)
                if stage == "LOGIN":
                    values = shlex.split(command)
                    assert values[1:] == ["qa@example.invalid", "amberize-synthetic-protocol-qa"]
                    send(f"{tag} OK LOGIN completed\r\n")
                elif stage == "CAPABILITY":
                    send(f"* CAPABILITY IMAP4rev1\r\n{tag} OK CAPABILITY completed\r\n")
                elif stage == "LIST":
                    send(f'* LIST (\\HasNoChildren) "/" "INBOX"\r\n{tag} OK LIST completed\r\n')
                elif stage == "SELECT":
                    if self.server.mode == "stall":
                        time.sleep(70)
                        return
                    send(f"* {len(self.server.messages)} EXISTS\r\n* FLAGS (\\Seen)\r\n"
                         f"* OK [UIDVALIDITY {self.server.generation}] generation\r\n"
                         f"* OK [UIDNEXT {max(self.server.messages, default=0)+1}] next\r\n"
                         f"{tag} OK [READ-WRITE] SELECT completed\r\n")
                elif upper.startswith("UID SEARCH "):
                    lower = int(re.search(r"UID (\d+):\*", upper[11:])[1])
                    # IMAP's star resolves to the highest existing UID, including
                    # the reversed range case when the cursor is already current.
                    if self.server.messages:
                        lower = min(lower, max(self.server.messages))
                    uids = sorted((uid for uid in self.server.messages if uid >= lower), reverse=True)
                    send(f"* SEARCH {' '.join(map(str,uids))}\r\n{tag} OK SEARCH completed\r\n")
                elif upper.startswith("UID FETCH "):
                    uid = int(upper.split()[2])
                    body = self.server.messages[uid]
                    if "RFC822.SIZE" in upper:
                        size = 51 * 1024 * 1024 if self.server.mode == "oversized" and uid == self.server.fail_uid else len(body)
                        send(f"* 1 FETCH (UID {uid} RFC822.SIZE {size})\r\n{tag} OK FETCH completed\r\n")
                    else:
                        self.server.body_requests.append(uid)
                        announced = 1024 * 1024 * 1024 if self.server.mode == "literal_bomb" and uid == self.server.fail_uid else len(body)
                        send(f'* 1 FETCH (UID {uid} FLAGS () INTERNALDATE "01-Oct-2026 00:00:00 +0000" BODY[] {{{announced}}}\r\n')
                        if uid == self.server.fail_uid and self.server.mode in ("interrupted", "literal_bomb"):
                            if self.server.mode == "interrupted":
                                send(body[:30])
                            self.connection.shutdown(socket.SHUT_RDWR)
                            return
                        send(body + f")\r\n{tag} OK FETCH completed\r\n".encode())
                elif stage == "LOGOUT":
                    send(f"* BYE QA complete\r\n{tag} OK LOGOUT completed\r\n")
                    return
                else:
                    raise AssertionError(f"Unexpected protocol stage {stage}")
        except (ConnectionError, OSError):
            return


def hashes(archive):
    with closing(sqlite3.connect(f"file:{archive}?mode=ro", uri=True)) as connection:
        result = {}
        for digest, raw in connection.execute("SELECT sha256,raw_mime FROM message_blobs"):
            assert hashlib.sha256(raw).hexdigest() == digest
            result[digest] = len(raw)
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--driver", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    assert os.environ.get("GITHUB_ACTIONS") == "true" and os.sys.platform.startswith("linux")
    root = args.root.resolve()
    assert root.is_relative_to(Path(os.environ["RUNNER_TEMP"]).resolve()) and not root.exists()
    root.mkdir(mode=0o700)
    cert, key = root / "tls-cert.pem", root / "tls-key.pem"
    subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                    "-keyout", str(key), "-out", str(cert), "-subj", "/CN=localhost",
                    "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost"], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    key.chmod(0o600)
    environment = dict(os.environ, SSL_CERT_FILE=str(cert))
    server = Provider(cert, key)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    port = server.server_address[1]
    archive = root / "archive.sqlite3"
    stages = []
    report = {"passed": False, "candidate_commit": "bcdb730817eae47a9d6b615a9182c1dbdcb900f7",
              "tls_certificate_validation_enabled": True, "trust_scope": "Test child processes only, through SSL_CERT_FILE",
              "secret_store": "In-memory synthetic credential; no OS Keychain changes", "stages": stages}

    def call(command, name, trusted=True):
        output = root / f"{name}-memory.txt"
        arguments = ["/usr/bin/time", "-v", "-o", str(output), str(args.driver), command, str(archive)]
        if command == "create":
            arguments.append(str(port))
        started = time.monotonic()
        completed = subprocess.run(arguments, check=True, capture_output=True, text=True,
                                   timeout=85, env=environment if trusted else
                                   dict(environment, SSL_CERT_FILE=ssl.get_default_verify_paths().cafile))
        value = json.loads(completed.stdout)
        value["duration_seconds"] = round(time.monotonic() - started, 3)
        value["peak_rss_kib"] = int(re.search(r"Maximum resident set size \(kbytes\): (\d+)", output.read_text())[1])
        assert value["integrity_ok"]
        (root / f"{name}.json").write_text(json.dumps(value, indent=2)+"\n")
        return value

    def expect(name, status, count, cursor, imported=None):
        value = call("sync", name)
        assert value["status"] == status, (name, value)
        assert value["blob_count"] == count, (name, value)
        assert value["mailboxes"][0]["cursor"] == cursor, (name, value)
        if imported is not None:
            assert value["messages_imported"] == imported, (name, value)
        stages.append({"name": name, "passed": True, "result": value})
        return value

    try:
        call("create", "create")
        rejected = call("sync", "untrusted-tls-certificate", trusted=False)
        assert rejected["status"] == "error" and rejected["blob_count"] == 0
        assert "tls handshake failed" in rejected["errors"][0]
        assert "LOGIN" not in server.requests
        stages.append({"name": "untrusted-tls-certificate-rejected-before-login", "passed": True, "result": rejected})
        expect("unsorted-search-and-attachments", "ok", 3, 3, 3)
        with closing(sqlite3.connect(f"file:{archive}?mode=ro", uri=True)) as connection:
            assert connection.execute("SELECT COUNT(*) FROM message_blobs WHERE has_attachments=1").fetchone()[0] == 3
        baseline = hashes(archive)
        expect("repeat-sync-no-duplicates", "ok", 3, 3, 0)
        server.messages.update({uid: mime(uid) for uid in (4, 5, 6)})
        server.mode, server.fail_uid = "interrupted", 5
        expect("interrupted-body-retains-cursor", "partial", 4, 4, 1)
        assert set(baseline) <= set(hashes(archive))
        server.mode = "normal"
        expect("reconnect-resumes-missing-uids", "ok", 6, 6, 2)
        server.messages.update({uid: mime(uid) for uid in (7, 8)})
        server.mode, server.fail_uid = "oversized", 7
        server.body_requests.clear()
        value = expect("oversized-preflight-retains-cursor", "partial", 6, 6, 0)
        assert not server.body_requests and "50 MiB" in value["errors"][0]
        del server.messages[7]
        server.mode = "normal"
        expect("oversized-resolution-resumes-next-uid", "ok", 7, 8, 1)
        server.messages[9] = mime(9)
        server.mode, server.fail_uid = "literal_bomb", 9
        value = expect("dishonest-size-one-gib-literal", "partial", 7, 8, 0)
        assert "50 MiB" in value["errors"][0] and value["peak_rss_kib"] < 150 * 1024
        server.mode = "normal"
        expect("literal-failure-retry", "ok", 8, 9, 1)
        server.generation = 11
        server.messages = {1: mime(1, 11), 2: mime(1, 10)}
        expect("uidvalidity-reset-and-content-deduplication", "ok", 9, 2, 2)
        server.mode = "stall"
        value = expect("server-stall-bounded-by-deadline", "partial", 9, 2, 0)
        assert 55 <= value["duration_seconds"] < 80 and "timed out" in value["errors"][0]
        server.mode = "normal"
        expect("recovery-after-stall", "ok", 9, 2, 0)
        server.shutdown()
        server.server_close()
        value = expect("provider-offline", "error", 9, 2)
        assert "connection refused" in value["errors"][0].lower()
        server = Provider(cert, key, port)
        server.generation = 11
        server.messages = {1: mime(1, 11), 2: mime(1, 10), 3: mime(3, 11)}
        threading.Thread(target=server.serve_forever, daemon=True).start()
        expect("provider-returns-and-delivers-new-message", "ok", 10, 3, 1)
        assert set(baseline) <= set(hashes(archive))
        report.update({"passed": True, "all_initial_mime_hashes_preserved": True,
                       "final_blob_count": len(hashes(archive)), "final_full_integrity_passed": True})
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        server.shutdown()
        server.server_close()
        (root / "provider-integration.json").write_text(json.dumps(report, indent=2)+"\n")
        cert.unlink(missing_ok=True)
        key.unlink(missing_ok=True)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
