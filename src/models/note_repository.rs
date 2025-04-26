use rusqlite::{Connection, OptionalExtension, Result};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::models::Note;

pub struct NoteRepository {
    connection: Arc<Mutex<Connection>>,
}

impl NoteRepository {
    pub fn new(connection: Arc<Mutex<Connection>>) -> Self {
        Self { connection }
    }

    pub fn create_note(&self, note: &Note) -> Result<()> {
        println!("Saving note to database with ID: {}", note.id);
        let mut connection = match self.connection.lock() {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("Failed to lock database connection: {}", e);
                return Err(rusqlite::Error::InvalidParameterName(
                    "Failed to lock connection".to_string(),
                ));
            }
        };

        let tx = connection.transaction()?;
        println!("Transaction started");

        let result = tx.execute(
            "INSERT INTO notes (id, title, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            [
                &note.id.to_string(),
                &note.title,
                &note.content,
                &note.created_at.to_string(),
            ],
        );

        match &result {
            Ok(rows) => println!("Inserted note successfully, {} rows affected", rows),
            Err(e) => {
                eprintln!("Error inserting note: {}", e);
                tx.rollback()?;
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(1),
                    Some(format!("Failed to insert: {}", e)),
                ));
            }
        }

        println!("Committing transaction");
        tx.commit()?;
        println!("Transaction committed");

        let _ = connection.execute("PRAGMA wal_checkpoint(FULL)", []);
        println!("Checkpoint completed");

        Ok(())
    }

    pub fn update_note(&self, note: &Note) -> Result<()> {
        let mut connection = match self.connection.lock() {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("Failed to lock database connection: {}", e);
                return Err(rusqlite::Error::InvalidParameterName(
                    "Failed to lock connection".to_string(),
                ));
            }
        };

        let tx = connection.transaction()?;

        let result = tx.execute(
            "UPDATE notes SET title = ?1, content = ?2, created_at = ?3 WHERE id = ?4",
            [
                &note.title,
                &note.content,
                &note.created_at.to_string(),
                &note.id.to_string(),
            ],
        );

        match &result {
            Ok(rows) => {
                if *rows == 0 {
                    eprintln!("Warning: No rows updated for note {}", note.id);
                } else {
                    println!("Updated note successfully, {} rows affected", rows);
                }
            }
            Err(e) => {
                eprintln!("Error updating note: {}", e);
                tx.rollback()?;
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(1),
                    Some(format!("Failed to update: {}", e)),
                ));
            }
        }

        tx.commit()?;

        let _ = connection.execute("PRAGMA wal_checkpoint(FULL)", []);

        Ok(())
    }

    pub fn delete_note(&self, id: &str) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute("DELETE FROM notes WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn get_note(&self, id: &str) -> Result<Option<Note>> {
        let connection = self.connection.lock().unwrap();
        let mut stmt =
            connection.prepare("SELECT id, title, content, created_at FROM notes WHERE id = ?1")?;

        let note = stmt
            .query_row([id], |row| {
                let id: String = row.get(0)?;
                let title: String = row.get(1)?;
                let content: String = row.get(2)?;

                let created_at: u64 = match row.get::<_, rusqlite::types::Value>(3)? {
                    rusqlite::types::Value::Integer(i) => i as u64,
                    rusqlite::types::Value::Real(f) => f as u64,
                    rusqlite::types::Value::Text(s) => s.parse().unwrap_or_default(),
                    _ => 0,
                };

                Ok(Note {
                    id: Uuid::parse_str(&id).unwrap_or_default(),
                    title,
                    content,
                    created_at,
                })
            })
            .optional()?;

        Ok(note)
    }

    pub fn list_notes(&self) -> Result<Vec<Note>> {
        let connection = self.connection.lock().unwrap();
        let mut stmt = connection
            .prepare("SELECT id, title, content, created_at FROM notes ORDER BY created_at DESC")?;

        let notes_iter = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let content: String = row.get(2)?;

            let created_at: u64 = match row.get::<_, rusqlite::types::Value>(3)? {
                rusqlite::types::Value::Integer(i) => i as u64,
                rusqlite::types::Value::Real(f) => f as u64,
                rusqlite::types::Value::Text(s) => s.parse().unwrap_or_default(),
                _ => 0,
            };

            Ok(Note {
                id: Uuid::parse_str(&id).unwrap_or_default(),
                title,
                content,
                created_at,
            })
        })?;

        let mut notes = Vec::new();
        for note_result in notes_iter {
            notes.push(note_result?);
        }

        Ok(notes)
    }
}
