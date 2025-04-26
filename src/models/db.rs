use rusqlite::{Connection, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::models::NoteRepository;

pub struct Database {
    connection: Arc<Mutex<Connection>>,
    pub notes: NoteRepository,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        println!("Opening database at: {:?}", path.as_ref());

        let path_for_logging = path.as_ref().to_path_buf();

        if let Ok(abs_path) = std::fs::canonicalize(path.as_ref()) {
            println!("Absolute database path: {:?}", abs_path);
        }

        let connection = Connection::open(path)?;

        let _ = connection.execute("PRAGMA synchronous = FULL", []);
        let _ = connection.execute("PRAGMA journal_mode = DELETE", []);
        let _ = connection.execute("PRAGMA foreign_keys = ON", []);
        println!("Database configured for reliability");

        match connection.execute(
            "CREATE TABLE IF NOT EXISTS notes (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL
            )",
            [],
        ) {
            Ok(_) => println!("Notes table created or already exists"),
            Err(e) => eprintln!("Error creating notes table: {}", e),
        }

        match connection.execute("PRAGMA user_version = 1", []) {
            Ok(_) => println!("Database is writable"),
            Err(e) => {
                println!("Warning: Database might not be writable: {}", e);
                println!("Checking file permissions...");

                if let Some(parent) = path_for_logging.parent() {
                    match std::fs::metadata(parent) {
                        Ok(metadata) => {
                            println!("Directory permissions: {:?}", metadata.permissions());
                        }
                        Err(e) => println!("Could not check directory permissions: {}", e),
                    }
                }
            }
        }

        let connection = Arc::new(Mutex::new(connection));

        let db = Self {
            notes: NoteRepository::new(Arc::clone(&connection)),
            connection,
        };

        if let Err(e) = db.migrate_database() {
            eprintln!("Warning: Database migration failed: {}", e);
        }

        Ok(db)
    }

    fn migrate_database(&self) -> Result<()> {
        println!("Checking if database migration is needed...");

        let needs_migration = {
            let connection = self.connection.lock().unwrap();
            let mut pragma_stmt = connection.prepare("PRAGMA table_info(notes)")?;
            let columns = pragma_stmt.query_map([], |row| {
                let name: String = row.get(1)?;
                let type_name: String = row.get(2)?;
                Ok((name, type_name))
            })?;

            let mut migration_needed = false;
            for column_result in columns {
                if let Ok((name, type_name)) = column_result {
                    if name == "created_at" && type_name != "INTEGER" {
                        println!(
                            "Column 'created_at' is of type '{}', needs migration to INTEGER",
                            type_name
                        );
                        migration_needed = true;
                        break;
                    }
                }
            }
            migration_needed
        };

        if needs_migration {
            println!("Starting database migration...");

            let mut connection = self.connection.lock().unwrap();
            let tx = connection.transaction()?;

            tx.execute(
                "CREATE TABLE notes_new (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                )",
                [],
            )?;

            tx.execute(
                "INSERT INTO notes_new SELECT id, title, content, CAST(created_at AS INTEGER) FROM notes",
                [],
            )?;

            tx.execute("DROP TABLE notes", [])?;

            tx.execute("ALTER TABLE notes_new RENAME TO notes", [])?;

            tx.commit()?;

            println!("Database migration completed successfully");
        } else {
            println!("No database migration needed");
        }

        Ok(())
    }
}
