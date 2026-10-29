use regex::Regex;
use rusqlite::Connection;
use std::{path::PathBuf, sync::LazyLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("DB Error: {0}")]
    DbError(#[from] rusqlite::Error),
}

pub type WordAndDefinition = (String, String);

pub trait Database {
    fn is_populated(&self) -> Result<bool, Error>;
    fn clear(&self) -> Result<(), Error>;
    fn populate_chunk(&self, word_and_definitions: Vec<WordAndDefinition>) -> Result<(), Error>;
}

pub struct SqliteDatabase {
    connection: Connection,
}

impl SqliteDatabase {
    pub fn build() -> Result<Self, Error> {
        Ok(Self {
            connection: create_connection()?,
        })
    }
}

fn db_path() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "db", "whatchamacallit.db"]
        .iter()
        .collect()
}

fn create_connection() -> Result<Connection, Error> {
    init_vec_extensions()?;

    let connection = Connection::open(db_path())?;
    init_tables(&connection)?;
    init_indices(&connection)?;

    Ok(connection)
}

fn init_vec_extensions() -> Result<(), Error> {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    }
    Ok(())
}

fn init_tables(connection: &Connection) -> Result<(), Error> {
    init_words(connection)?;
    init_word_embeddings(connection)
}

fn init_word_embeddings(connection: &Connection) -> Result<(), Error> {
    let sql = "
CREATE VIRTUAL TABLE IF NOT EXISTS word_embeddings USING vec0(
    id INTEGER PRIMARY KEY,
    embedding FLOAT[384]
);";

    connection.execute(sql, [])?;
    Ok(())
}

fn init_words(connection: &Connection) -> Result<(), Error> {
    let sql = "
CREATE TABLE IF NOT EXISTS words (
    id               INTEGER  PRIMARY KEY  AUTOINCREMENT,
    word             TEXT     UNIQUE       NOT NULL,
    text_definition  TEXT     NOT NULL
);";

    connection.execute(sql, [])?;
    Ok(())
}

fn init_indices(connection: &Connection) -> Result<(), Error> {
    let create_words_word_index = "CREATE INDEX IF NOT EXISTS words_word ON words (word);";

    connection.execute(create_words_word_index, [])?;
    Ok(())
}

impl Database for SqliteDatabase {
    fn is_populated(&self) -> Result<bool, Error> {
        let pop: u32 = self
            .connection
            .query_row("SELECT COUNT(1) FROM words", [], |row| row.get(0))?;

        Ok(pop > 0)
    }

    fn clear(&self) -> Result<(), Error> {
        let sql = "TRUNCATE TABLE words;  TRUNCATE TABLE word_embeddings;";
        self.connection.execute(sql, [])?;
        Ok(())
    }

    fn populate_chunk(&self, word_and_definitions: Vec<WordAndDefinition>) -> Result<(), Error> {
        let values = word_and_definitions
            .iter()
            .filter_map(|(word, def)| {
                let word = escape_for_sqlite(word);
                let def = escape_for_sqlite(def);

                if word.is_empty() || def.is_empty() {
                    return None;
                }

                Some(format!("('{word}', '{def}')"))
            })
            .collect::<Vec<_>>()
            .join(" ");

        let sql = format!("INSERT INTO words (word, text_definition) VALUES {values};");
        self.connection.execute(&sql, [])?;

        Ok(())
    }
}

fn control_characters_pattern() -> &'static Regex {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{Cc}").unwrap());
    &PATTERN
}

fn escape_for_sqlite(text: &str) -> String {
    let result = control_characters_pattern().replace_all(text, "");
    let result = result.replace("'", "''");
    result.trim().to_string()
}
