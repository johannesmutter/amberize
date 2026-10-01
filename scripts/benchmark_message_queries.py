"""Reproducible synthetic SQLite comparison; never opens a user's archive.

Run: python3 scripts/benchmark_message_queries.py --sizes 10000 100000 1000000
Measures list-query shapes with small MIME bodies and warm database caches.
Results exclude IPC, migration, ingestion, native memory and network costs.
"""
import argparse
import json
import sqlite3
import statistics
import tempfile
import time
from pathlib import Path

SELECT = """SELECT ml.id,ml.message_blob_id,mb.subject,mb.from_address,
coalesce(mb.date_header,ml.internal_date),substr(coalesce(mb.body_text,''),1,200),
a.id,a.email_address,m.id,m.imap_name
FROM message_locations ml JOIN message_blobs mb ON mb.id=ml.message_blob_id
JOIN mailboxes m ON m.id=ml.mailbox_id JOIN accounts a ON a.id=ml.account_id"""
OLD = SELECT + """ WHERE ml.gone_from_server_at IS NULL AND a.disabled=0
AND (? IS NULL OR m.imap_name=? COLLATE NOCASE) AND (? IS NULL OR ml.account_id=?)
ORDER BY coalesce(ml.internal_date,mb.date_header,mb.imported_at) DESC,ml.id DESC
LIMIT 100 OFFSET ?"""
# The real new query also returns timestamp/attachment columns.
NEW = SELECT.replace("\nFROM", ",ml.sort_timestamp,mb.has_attachments\nFROM")


def measure(conn, sql, args, repeats):
    conn.execute(sql, args).fetchall()
    durations = []
    for _ in range(repeats):
        start = time.perf_counter()
        rows = conn.execute(sql, args).fetchall()
        durations.append((time.perf_counter() - start) * 1000)
    return {
        "median_ms": round(statistics.median(durations), 3),
        "rows": len(rows),
        "plan": [r[3] for r in conn.execute("EXPLAIN QUERY PLAN " + sql, args)],
    }


def benchmark(size, repeats):
    with tempfile.TemporaryDirectory(prefix="amberize-query-benchmark-") as directory:
        conn = sqlite3.connect(Path(directory) / "synthetic.sqlite3")
        conn.executescript("""
        PRAGMA journal_mode=WAL; PRAGMA cache_size=-4096;
        CREATE TABLE accounts(id INTEGER PRIMARY KEY,email_address TEXT,disabled INTEGER);
        CREATE TABLE mailboxes(id INTEGER PRIMARY KEY,imap_name TEXT);
        CREATE TABLE message_blobs(id INTEGER PRIMARY KEY,subject TEXT,from_address TEXT,
          date_header TEXT,imported_at TEXT,body_text TEXT,has_attachments INTEGER);
        CREATE TABLE message_locations(id INTEGER PRIMARY KEY,message_blob_id INTEGER,
          account_id INTEGER,mailbox_id INTEGER,internal_date TEXT,gone_from_server_at TEXT,
          sort_timestamp INTEGER);
        CREATE INDEX idx_locations_sort ON message_locations(sort_timestamp,id);
        CREATE INDEX idx_locations_account_sort ON message_locations(account_id,sort_timestamp,id);
        INSERT INTO accounts VALUES(1,'a@example.com',0),(2,'b@example.com',0);
        INSERT INTO mailboxes VALUES(1,'INBOX');
        """)
        conn.executemany("INSERT INTO message_blobs VALUES(?,?,'sender',NULL,?,'body',?)",
                         ((i, f"Subject {i}", f"{i:012d}", i % 10 == 0) for i in range(1,size+1)))
        conn.executemany("INSERT INTO message_locations VALUES(?,?,?,1,NULL,NULL,?)",
                         ((i,i,i % 2 + 1,i) for i in range(1,size+1)))
        conn.commit()
        boundary = size // 2
        old_args = (None,None,None,None)
        results = {
            "old_first": measure(conn, OLD, (*old_args,0), repeats),
            "old_deep_offset": measure(conn, OLD, (*old_args,boundary), repeats),
            "new_first": measure(conn, NEW+" WHERE 1=1 ORDER BY ml.sort_timestamp DESC,ml.id DESC LIMIT 100 OFFSET 0", (), repeats),
            "new_deep_cursor": measure(conn, NEW+" WHERE (ml.sort_timestamp,ml.id)<(?,?) ORDER BY ml.sort_timestamp DESC,ml.id DESC LIMIT 100 OFFSET 0", (boundary+1,boundary+1), repeats),
            "new_account_date": measure(conn, NEW+" WHERE ml.account_id=? AND ml.sort_timestamp>=? AND ml.sort_timestamp<? ORDER BY ml.sort_timestamp DESC,ml.id DESC LIMIT 100 OFFSET 0", (1,boundary,size), repeats),
            "new_attachments": measure(conn, NEW+" WHERE mb.has_attachments=1 ORDER BY ml.sort_timestamp DESC,ml.id DESC LIMIT 100 OFFSET 0", (), repeats),
        }
        conn.close()
        return {"locations":size,"queries":results}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sizes", nargs="+", type=int, default=[10000,100000,1000000])
    parser.add_argument("--repeats", type=int, default=5)
    options = parser.parse_args()
    if options.repeats < 1 or any(size < 200 for size in options.sizes):
        parser.error("positive repetitions and at least 200 locations are required")
    print(json.dumps({"sqlite":sqlite3.sqlite_version,"repeats":options.repeats,
                      "results":[benchmark(s,options.repeats) for s in options.sizes]},indent=2))
