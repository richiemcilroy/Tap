pub mod macos_menu;

use lazy_static::lazy_static;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

lazy_static! {
    pub static ref NOTE_TO_DELETE: Mutex<Option<Uuid>> = Mutex::new(None);
}

pub fn get_db_path() -> PathBuf {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home_dir.join(".tap").join("notes.db")
}

pub fn dump_db_contents() -> Result<(), io::Error> {
    let db_path = get_db_path();
    println!("Database path: {:?}", db_path);

    if !db_path.exists() {
        println!("Database file does not exist yet!");
        return Ok(());
    }

    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to open DB: {}", e)))?;

    let mut stmt = conn
        .prepare("SELECT id, title, content, created_at FROM notes")
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to prepare query: {}", e),
            )
        })?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to query: {}", e)))?;

    println!("Dumping database contents:");
    for (i, row_result) in rows.enumerate() {
        match row_result {
            Ok((id, title, content, created_at)) => {
                println!("Note {}:", i + 1);
                println!("  ID: {}", id);
                println!("  Title: {}", title);
                println!(
                    "  Content: {} (truncated)",
                    content.chars().take(30).collect::<String>()
                );
                println!("  Created: {}", created_at);
                println!();
            }
            Err(e) => println!("Error reading row: {}", e),
        }
    }

    Ok(())
}
