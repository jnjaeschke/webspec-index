//! The refresh lock: one refresh or effects rebuild at a time across processes.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::cell::RefCell;

/// A lock older than this is reclaimable even when its pid is alive.
pub const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(120);

/// A claimed `refresh_lock` row; dropping it releases the lock.
pub struct RefreshGuard<'c> {
    conn: &'c Connection,
    pid: u32,
    started_at: RefCell<String>,
}

impl RefreshGuard<'_> {
    /// Restart the lock's age at `now`, so a refresh running longer than
    /// [`STALE_AFTER`] keeps it.
    pub fn touch(&self, now: DateTime<Utc>) -> anyhow::Result<()> {
        let touched = now.to_rfc3339();
        self.conn.execute(
            "UPDATE refresh_lock SET started_at=?3 WHERE id=1 AND pid=?1 AND started_at=?2",
            (self.pid, &*self.started_at.borrow(), &touched),
        )?;
        *self.started_at.borrow_mut() = touched;
        Ok(())
    }
}

impl Drop for RefreshGuard<'_> {
    fn drop(&mut self) {
        let _ = self.conn.execute(
            "DELETE FROM refresh_lock WHERE id=1 AND pid=?1 AND started_at=?2",
            (self.pid, &*self.started_at.borrow()),
        );
    }
}

/// Claim the lock for `pid` when it is free, older than [`STALE_AFTER`] or
/// held by a dead process; `None` while another refresh holds it.
pub fn try_claim<'c>(
    conn: &'c Connection,
    pid: u32,
    now: DateTime<Utc>,
) -> anyhow::Result<Option<RefreshGuard<'c>>> {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    if let Some((holder, started_at)) = read_holder(&tx)? {
        let stale = started_at.is_none_or(|started_at| {
            now.signed_duration_since(started_at)
                .to_std()
                .is_ok_and(|age| age > STALE_AFTER)
        });
        if !stale && pid_is_alive(holder) {
            return Ok(None);
        }
    }
    let started_at = now.to_rfc3339();
    tx.execute(
        "INSERT OR REPLACE INTO refresh_lock(id, pid, started_at) VALUES (1, ?1, ?2)",
        (pid, &started_at),
    )?;
    tx.commit()?;
    Ok(Some(RefreshGuard {
        conn,
        pid,
        started_at: RefCell::new(started_at),
    }))
}

/// The pid holding the lock and when it claimed it.
pub fn holder(conn: &Connection) -> anyhow::Result<Option<(u32, DateTime<Utc>)>> {
    Ok(read_holder(conn)?
        .map(|(pid, started_at)| (pid, started_at.unwrap_or(DateTime::<Utc>::MIN_UTC))))
}

/// The holder's pid and claim time; an unparsable time is `None`.
fn read_holder(conn: &Connection) -> anyhow::Result<Option<(u32, Option<DateTime<Utc>>)>> {
    let row: Option<(u32, String)> = conn
        .query_row(
            "SELECT pid, started_at FROM refresh_lock WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row.map(|(pid, started_at)| {
        let started_at = DateTime::parse_from_rfc3339(&started_at)
            .ok()
            .map(|t| t.with_timezone(&Utc));
        (pid, started_at)
    }))
}

/// Whether a process with `pid` exists (possibly owned by another user).
#[cfg(unix)]
pub fn pid_is_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 performs only the existence and permission checks.
    let signalled = unsafe { libc::kill(pid, 0) } == 0;
    signalled || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Without a cheap liveness probe, a holder counts as alive until its lock goes
/// stale.
#[cfg(not(unix))]
pub fn pid_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_release_and_contention() {
        let conn = crate::db::open_test_db().unwrap();
        let now = chrono::Utc::now();
        let guard = try_claim(&conn, std::process::id(), now)
            .unwrap()
            .expect("free lock");
        assert!(
            try_claim(&conn, std::process::id() + 1, now)
                .unwrap()
                .is_none(),
            "held by a live pid"
        );
        drop(guard);
        assert!(holder(&conn).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn dead_pid_lock_is_reclaimed() {
        let conn = crate::db::open_test_db().unwrap();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        conn.execute(
            "INSERT INTO refresh_lock(id, pid, started_at) VALUES (1, ?1, ?2)",
            (dead, chrono::Utc::now().to_rfc3339()),
        )
        .unwrap();
        assert!(try_claim(&conn, std::process::id(), chrono::Utc::now())
            .unwrap()
            .is_some());
    }

    #[test]
    fn touched_lock_is_not_reclaimed() {
        let conn = crate::db::open_test_db().unwrap();
        let now = chrono::Utc::now();
        let guard = try_claim(
            &conn,
            std::process::id(),
            now - chrono::Duration::seconds(121),
        )
        .unwrap()
        .unwrap();
        guard.touch(now).unwrap();
        assert!(try_claim(&conn, std::process::id() + 1, now)
            .unwrap()
            .is_none());
        drop(guard);
        assert!(holder(&conn).unwrap().is_none(), "released after a touch");
    }

    #[test]
    fn old_lock_is_reclaimed_even_from_a_live_pid() {
        let conn = crate::db::open_test_db().unwrap();
        let old = chrono::Utc::now() - chrono::Duration::seconds(121);
        conn.execute(
            "INSERT INTO refresh_lock(id, pid, started_at) VALUES (1, ?1, ?2)",
            (std::process::id(), old.to_rfc3339()),
        )
        .unwrap();
        assert!(try_claim(&conn, std::process::id() + 1, chrono::Utc::now())
            .unwrap()
            .is_some());
    }
}
