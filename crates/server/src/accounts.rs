//! Accounts and login sessions.
//!
//! Passwords are hashed with argon2id (random salt, PHC string). A login
//! returns a random 256-bit bearer token. Sessions live in memory only, so a
//! restart logs everyone out. Each account owns one [`PlayerId`], assigned in
//! registration order starting at 1 (0 is the wild ecosystem).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use evo_sim::PlayerId;
use rand::RngCore;

use crate::store::{AccountRecord, Store};

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    BadName,
    WeakPassword,
    NameTaken,
    BadCredentials,
    Internal(String),
}

impl AuthError {
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::BadName => "bad_name",
            AuthError::WeakPassword => "weak_password",
            AuthError::NameTaken => "name_taken",
            AuthError::BadCredentials => "bad_credentials",
            AuthError::Internal(_) => "internal",
        }
    }

    pub fn message(&self) -> String {
        match self {
            AuthError::BadName => "names are 3 to 24 letters, digits, '_' or '-'".into(),
            AuthError::WeakPassword => format!("passwords need at least {MIN_PASSWORD} characters"),
            AuthError::NameTaken => "that name is taken".into(),
            AuthError::BadCredentials => "wrong name or password".into(),
            AuthError::Internal(e) => e.clone(),
        }
    }
}

pub const MIN_PASSWORD: usize = 8;

#[derive(Clone, Debug)]
pub struct Session {
    pub player: PlayerId,
    pub name: String,
}

pub struct Accounts {
    store: Arc<dyn Store>,
    inner: Mutex<Inner>,
}

struct Inner {
    by_name: HashMap<String, AccountRecord>,
    /// Names reserved by registrations still hashing or persisting.
    pending: Vec<String>,
    sessions: HashMap<String, Session>,
    next_player: PlayerId,
}

fn valid_name(name: &str) -> bool {
    (3..=24).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Accounts {
    pub fn new(store: Arc<dyn Store>, existing: Vec<AccountRecord>) -> Self {
        let next_player = existing.iter().map(|a| a.player).max().unwrap_or(0) + 1;
        let by_name = existing.into_iter().map(|a| (a.name.clone(), a)).collect();
        Self { store, inner: Mutex::new(Inner { by_name, pending: Vec::new(), sessions: HashMap::new(), next_player }) }
    }

    pub fn name_of(&self, player: PlayerId) -> Option<String> {
        let g = self.inner.lock().unwrap();
        g.by_name.values().find(|a| a.player == player).map(|a| a.name.clone())
    }

    /// Create an account and log it in.
    pub async fn register(&self, name: &str, password: &str) -> Result<(String, Session), AuthError> {
        if !valid_name(name) {
            return Err(AuthError::BadName);
        }
        if password.chars().count() < MIN_PASSWORD {
            return Err(AuthError::WeakPassword);
        }
        let player = {
            let mut g = self.inner.lock().unwrap();
            if g.by_name.contains_key(name) || g.pending.iter().any(|n| n == name) {
                return Err(AuthError::NameTaken);
            }
            g.pending.push(name.to_string());
            let p = g.next_player;
            g.next_player += 1;
            p
        };
        let result = self.finish_register(name, password, player).await;
        let mut g = self.inner.lock().unwrap();
        g.pending.retain(|n| n != name);
        let record = result?;
        g.by_name.insert(name.to_string(), record);
        let session = Session { player, name: name.to_string() };
        let token = new_token();
        g.sessions.insert(token.clone(), session.clone());
        Ok((token, session))
    }

    async fn finish_register(&self, name: &str, password: &str, player: PlayerId) -> Result<AccountRecord, AuthError> {
        let password = password.to_string();
        let password_hash = tokio::task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string())
        })
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?
        .map_err(|e| AuthError::Internal(e.to_string()))?;
        let created_unix = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let record = AccountRecord { name: name.to_string(), player, password_hash, created_unix };
        self.store.insert_account(&record).await.map_err(|e| AuthError::Internal(e.to_string()))?;
        Ok(record)
    }

    pub async fn login(&self, name: &str, password: &str) -> Result<(String, Session), AuthError> {
        let record = self.inner.lock().unwrap().by_name.get(name).cloned().ok_or(AuthError::BadCredentials)?;
        let password = password.to_string();
        let hash = record.password_hash.clone();
        let ok = tokio::task::spawn_blocking(move || {
            PasswordHash::new(&hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
        })
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;
        if !ok {
            return Err(AuthError::BadCredentials);
        }
        let session = Session { player: record.player, name: record.name };
        let token = new_token();
        self.inner.lock().unwrap().sessions.insert(token.clone(), session.clone());
        Ok((token, session))
    }

    pub fn session(&self, token: &str) -> Option<Session> {
        self.inner.lock().unwrap().sessions.get(token).cloned()
    }

    pub fn logout(&self, token: &str) {
        self.inner.lock().unwrap().sessions.remove(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    #[tokio::test]
    async fn register_login_and_sessions() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::default());
        let acc = Accounts::new(store.clone(), vec![]);
        let (t1, s1) = acc.register("alice", "correct horse").await.unwrap();
        assert_eq!(s1.player, 1);
        assert_eq!(acc.register("alice", "another one").await.unwrap_err(), AuthError::NameTaken);
        assert_eq!(acc.register("a!", "whatever123").await.unwrap_err(), AuthError::BadName);
        assert_eq!(acc.register("bob", "short").await.unwrap_err(), AuthError::WeakPassword);
        let (_, s2) = acc.register("bob", "bob's password").await.unwrap();
        assert_eq!(s2.player, 2);
        assert_eq!(acc.login("alice", "wrong password").await.unwrap_err(), AuthError::BadCredentials);
        let (t3, s3) = acc.login("alice", "correct horse").await.unwrap();
        assert_eq!(s3.player, 1);
        assert_ne!(t1, t3);
        assert_eq!(acc.session(&t3).unwrap().name, "alice");
        // Persisted hashes survive a restart.
        let stored = store.load_accounts().await.unwrap();
        assert!(stored[0].password_hash.starts_with("$argon2"));
        let acc2 = Accounts::new(store, stored);
        assert!(acc2.session(&t3).is_none());
        assert_eq!(acc2.login("bob", "bob's password").await.unwrap().1.player, 2);
        assert_eq!(acc2.register("carol", "carol's password").await.unwrap().1.player, 3);
    }
}
