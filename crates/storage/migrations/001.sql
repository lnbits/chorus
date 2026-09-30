BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
INSERT OR IGNORE INTO schema_migrations VALUES(1, unixepoch());
-- Typed, versioned JSON is storage only. Consensus hashes always use canonical Borsh.
CREATE TABLE IF NOT EXISTS records(kind TEXT NOT NULL,id TEXT NOT NULL,federation TEXT,value TEXT NOT NULL,created_at INTEGER NOT NULL,PRIMARY KEY(kind,id));
CREATE INDEX IF NOT EXISTS records_federation ON records(kind,federation,created_at);
CREATE TRIGGER IF NOT EXISTS records_no_update BEFORE UPDATE ON records BEGIN SELECT RAISE(ABORT,'immutable history'); END;
CREATE TRIGGER IF NOT EXISTS records_no_delete BEFORE DELETE ON records BEGIN SELECT RAISE(ABORT,'immutable history'); END;
CREATE TABLE IF NOT EXISTS blobs(hash TEXT PRIMARY KEY,bytes BLOB NOT NULL);
CREATE TRIGGER IF NOT EXISTS blobs_no_update BEFORE UPDATE ON blobs BEGIN SELECT RAISE(ABORT,'immutable blob'); END;
CREATE TABLE IF NOT EXISTS signing_guards(identity TEXT NOT NULL,scope TEXT NOT NULL,commitment TEXT NOT NULL,PRIMARY KEY(identity,scope));
CREATE TRIGGER IF NOT EXISTS guard_no_update BEFORE UPDATE ON signing_guards BEGIN SELECT RAISE(ABORT,'immutable signing guard'); END;
CREATE TRIGGER IF NOT EXISTS guard_no_delete BEFORE DELETE ON signing_guards BEGIN SELECT RAISE(ABORT,'immutable signing guard'); END;
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS activity(id INTEGER PRIMARY KEY,kind TEXT NOT NULL,federation TEXT,target TEXT NOT NULL,message TEXT NOT NULL,created_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS admin(id INTEGER PRIMARY KEY CHECK(id=1),username TEXT NOT NULL,password_hash TEXT NOT NULL,failures INTEGER NOT NULL DEFAULT 0,blocked_until INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS sessions(token_hash TEXT PRIMARY KEY,csrf TEXT NOT NULL,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS outbox(event_id TEXT PRIMARY KEY,event TEXT NOT NULL,recipients TEXT NOT NULL,sent INTEGER NOT NULL DEFAULT 0);
COMMIT;
